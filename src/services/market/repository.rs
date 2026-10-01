//! Cache candle `/klines` berbasis rentang waktu: semua candle yang SUDAH CLOSE disimpan di 1
//! sorted set per simbol+interval (score = `open_time`), dipakai bersama oleh semua request apa
//! pun panjang/posisi rentangnya. Candle closed tidak pernah berubah, jadi tiap candle cukup
//! diambil dari exchange sekali — beban ke exchange sebanding dengan data unik, bukan jumlah user.
//!
//! Semua operasi best-effort: Redis error/down dianggap cache miss, tidak pernah menggagalkan request.

use std::time::Duration;

use deadpool_redis::redis::{self, AsyncCommands};

use super::types::Candle;
use crate::database::RedisPool;

/// Key yang tidak diakses selama ini dibuang otomatis (diperpanjang tiap kali dibaca/ditulis),
/// supaya kombinasi simbol+interval yang jarang dibuka tidak memakan memori Redis selamanya.
const IDLE_TTL_SECONDS: i64 = 24 * 60 * 60;
/// Lock anti-stampede: 1 request mengambil rentang yang sama dari exchange, sisanya menunggu hasilnya.
const LOCK_TTL_MS: u64 = 10_000;
const LOCK_WAIT_STEP: Duration = Duration::from_millis(100);
const LOCK_WAIT_STEPS: u32 = 30;
/// Candle live di-update stream tiap ±1-2 detik. Kalau lewat dari ini tanpa update, artinya
/// sedang tidak ada stream untuk simbol+interval itu -> key hilang, request kembali ke exchange.
const LIVE_TTL_SECONDS: u64 = 10;

fn range_key(symbol: &str, interval: &str) -> String {
    format!("klines:range:{symbol}:{interval}")
}

/// Candle yang sedang berjalan (belum close), ditimpa tiap event stream.
fn live_key(symbol: &str, interval: &str) -> String {
    format!("klines:live:{symbol}:{interval}")
}

/// `open_time` candle PERTAMA simbol ini di exchange (awal listing) — tanpa penanda ini, halaman
/// histori paling awal (isinya < `limit` candle) tidak pernah bisa dianggap lengkap.
fn first_key(symbol: &str, interval: &str) -> String {
    format!("klines:first:{symbol}:{interval}")
}

fn lock_key(symbol: &str, interval: &str, end_time: i64, limit: u16) -> String {
    format!("klines:lock:{symbol}:{interval}:{end_time}:{limit}")
}

/// Durasi 1 candle dalam ms. `None` untuk `1M` (panjang bulan tidak tetap) — interval itu tidak
/// memakai cache rentang (cek kesinambungan butuh durasi yang tetap).
pub fn interval_ms(interval: &str) -> Option<i64> {
    const MINUTE: i64 = 60 * 1000;
    let minutes = match interval {
        "1m" => 1,
        "3m" => 3,
        "5m" => 5,
        "15m" => 15,
        "30m" => 30,
        "1h" => 60,
        "2h" => 120,
        "4h" => 240,
        "6h" => 360,
        "8h" => 480,
        "12h" => 720,
        "1d" => 1440,
        "3d" => 3 * 1440,
        "1w" => 7 * 1440,
        _ => return None,
    };
    Some(minutes * MINUTE)
}

/// Ambil `limit` candle terakhir dengan `open_time <= end_time` dari cache — sama seperti
/// `/api/v3/klines?endTime=...`, terurut lama->baru. `None` (cache miss) kalau rentangnya belum
/// lengkap: kurang candle, ada yang bolong, atau candle teratas bukan yang terakhir sebelum `end_time`.
pub async fn read_range(
    redis: &RedisPool,
    symbol: &str,
    interval: &str,
    end_time: i64,
    limit: u16,
) -> Option<Vec<Candle>> {
    let step = interval_ms(interval)?;
    let mut conn = redis.get().await.ok()?;
    let key = range_key(symbol, interval);

    let raw: Vec<String> = conn
        .zrevrangebyscore_limit(&key, end_time, "-inf", 0, limit as isize)
        .await
        .ok()?;
    if raw.is_empty() {
        return None;
    }

    let mut candles: Vec<Candle> = raw
        .iter()
        .filter_map(|member| serde_json::from_str(member).ok())
        .collect();
    candles.dedup_by_key(|c| c.open_time);

    // Candle teratas harus candle terakhir sebelum end_time — kalau candle berikutnya juga
    // <= end_time, berarti ada yang belum tersimpan di atasnya.
    if candles[0].open_time + step <= end_time {
        return None;
    }
    if candles.windows(2).any(|w| w[0].open_time - w[1].open_time != step) {
        return None;
    }

    if candles.len() < limit as usize {
        let first: Option<i64> = conn.get(first_key(symbol, interval)).await.ok()?;
        if first != candles.last().map(|c| c.open_time) {
            return None;
        }
    }

    let _: Result<(), _> = conn.expire(&key, IDLE_TTL_SECONDS).await;
    candles.reverse();
    Some(candles)
}

/// Simpan candle yang sudah close (`close_time < now`) ke sorted set. `reached_listing_start`
/// = respons exchange berisi lebih sedikit dari yang diminta, artinya candle tertua di sini adalah
/// candle pertama simbol ini.
pub async fn store_closed(
    redis: &RedisPool,
    symbol: &str,
    interval: &str,
    candles: &[Candle],
    reached_listing_start: bool,
) {
    if interval_ms(interval).is_none() || candles.is_empty() {
        return;
    }
    let Ok(mut conn) = redis.get().await else {
        return;
    };

    let now_ms = chrono::Utc::now().timestamp_millis();
    let members: Vec<(i64, String)> = candles
        .iter()
        .filter(|c| c.close_time < now_ms)
        .filter_map(|c| Some((c.open_time, serde_json::to_string(c).ok()?)))
        .collect();
    if members.is_empty() {
        return;
    }

    let key = range_key(symbol, interval);
    let _: Result<(), _> = conn.zadd_multiple(&key, &members).await;
    let _: Result<(), _> = conn.expire(&key, IDLE_TTL_SECONDS).await;

    if reached_listing_start {
        let first = first_key(symbol, interval);
        let _: Result<(), _> = conn.set_ex(&first, members[0].0, IDLE_TTL_SECONDS as u64).await;
    }
}

/// Simpan candle yang sedang berjalan (dari stream). TTL pendek: key ini hanya ada selama stream
/// untuk simbol+interval itu hidup, jadi keberadaannya sekaligus tanda datanya segar.
pub async fn store_live(redis: &RedisPool, symbol: &str, interval: &str, candle: &Candle) {
    let (Ok(mut conn), Ok(payload)) = (redis.get().await, serde_json::to_string(candle)) else {
        return;
    };
    let _: Result<(), _> = conn
        .set_ex(live_key(symbol, interval), payload, LIVE_TTL_SECONDS)
        .await;
}

/// Halaman chart terbaru langsung dari Redis: `limit - 1` candle closed + candle yang sedang
/// berjalan dari stream. `None` kalau tidak ada stream aktif (candle live tidak ada/basi) atau
/// histori di bawahnya belum lengkap — pemanggil kembali mengambil dari exchange.
pub async fn read_latest(
    redis: &RedisPool,
    symbol: &str,
    interval: &str,
    limit: u16,
) -> Option<Vec<Candle>> {
    interval_ms(interval)?;
    let mut conn = redis.get().await.ok()?;
    let raw: Option<String> = conn.get(live_key(symbol, interval)).await.ok()?;
    let live: Candle = serde_json::from_str(&raw?).ok()?;
    drop(conn);

    let mut candles = if limit > 1 {
        read_range(redis, symbol, interval, live.open_time - 1, limit - 1).await?
    } else {
        Vec::new()
    };
    candles.push(live);
    Some(candles)
}

/// Coba ambil lock rentang ini. `true` = pemanggil yang bertugas mengambil dari exchange.
/// Redis error dianggap dapat lock (lebih baik request ke exchange daripada macet).
pub async fn try_lock(redis: &RedisPool, symbol: &str, interval: &str, end_time: i64, limit: u16) -> bool {
    let Ok(mut conn) = redis.get().await else {
        return true;
    };
    let result: redis::RedisResult<Option<String>> = redis::cmd("SET")
        .arg(lock_key(symbol, interval, end_time, limit))
        .arg(1)
        .arg("NX")
        .arg("PX")
        .arg(LOCK_TTL_MS)
        .query_async(&mut conn)
        .await;
    !matches!(result, Ok(None))
}

pub async fn unlock(redis: &RedisPool, symbol: &str, interval: &str, end_time: i64, limit: u16) {
    if let Ok(mut conn) = redis.get().await {
        let _: Result<(), _> = conn.del(lock_key(symbol, interval, end_time, limit)).await;
    }
}

/// Request lain sedang mengambil rentang yang sama — tunggu sebentar sampai hasilnya masuk cache.
pub async fn wait_for_range(
    redis: &RedisPool,
    symbol: &str,
    interval: &str,
    end_time: i64,
    limit: u16,
) -> Option<Vec<Candle>> {
    for _ in 0..LOCK_WAIT_STEPS {
        tokio::time::sleep(LOCK_WAIT_STEP).await;
        if let Some(candles) = read_range(redis, symbol, interval, end_time, limit).await {
            return Some(candles);
        }
    }
    None
}
