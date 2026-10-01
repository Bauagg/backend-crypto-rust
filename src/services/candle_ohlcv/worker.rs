use std::str::FromStr;
use std::time::{Duration, Instant};

use futures_util::stream::{self, StreamExt};
use rust_decimal::Decimal;
use sqlx::PgPool;

use super::repository::{find_latest_open_times, insert_candles};
use super::types::NewCandle;
use crate::clients::binance;
use crate::services::flex_params::repository::find_flex_param_values_with_recently_deleted;
use crate::utils::app_error::AppError;

const SYMBOL_TYPE_PARAM: &str = "SIMBOL_CRYPTO";
/// Strategi swing trading pakai timeframe 1D — worker fokus kumpulkan candle harian.
const COLLECTOR_INTERVAL: &str = "1d";
const DAY_MS: i64 = 24 * 60 * 60 * 1000;
/// Histori awal untuk simbol yang belum punya data sama sekali (~1,4 tahun candle harian).
const INITIAL_HISTORY_LIMIT: u16 = 500;
/// Batas maksimal /api/v3/klines per request — cukup untuk mengejar ketertinggalan data.
const CATCH_UP_LIMIT: u16 = 1000;
/// Sync tiap 30 menit, disejajarkan ke jam (xx:00 & xx:30 UTC) + `SYNC_OFFSET` — candle harian
/// close 00:00 UTC, jadi candle kemarin sudah masuk DB ±1 menit setelahnya (bot Python memanggil
/// `/signal` setelah candle kemarin tersedia).
const SYNC_INTERVAL_SECS: i64 = 30 * 60;
/// Jeda kecil setelah pergantian slot supaya exchange sudah memfinalkan candle yang baru close.
const SYNC_OFFSET_SECS: i64 = 30;
/// Simbol yang di-fetch bersamaan. /klines berbobot 2 dari batas 6000/menit Binance, jadi 8
/// request paralel masih jauh dari rate limit, tapi memangkas waktu backfill ratusan simbol.
const CONCURRENCY: usize = 8;
/// Coin yang di-soft-delete tetap dikumpulkan candle-nya selama ini — riwayat rekomendasi Python
/// (`/recommendations/momentum/history`) butuh harga coin yang sudah keluar dari daftar.
const KEEP_DELETED_DAYS: i32 = 30;

/// Durasi sampai slot sync berikutnya (xx:00:30 / xx:30:30 UTC).
fn until_next_slot() -> Duration {
    let now = chrono::Utc::now().timestamp();
    let next = (now - SYNC_OFFSET_SECS).div_euclid(SYNC_INTERVAL_SECS) * SYNC_INTERVAL_SECS
        + SYNC_INTERVAL_SECS
        + SYNC_OFFSET_SECS;
    Duration::from_secs((next - now).max(1) as u64)
}

/// `open_time` candle harian terakhir yang sudah close (kemarin 00:00 UTC). Simbol yang sudah
/// menyimpan candle ini tidak perlu di-fetch — belum ada candle baru sampai 00:00 UTC berikutnya.
fn last_closed_open_time(now_ms: i64) -> i64 {
    now_ms.div_euclid(DAY_MS) * DAY_MS - DAY_MS
}

/// Jalan sebagai background task selama server hidup: sync langsung saat start, lalu tiap slot
/// 30 menit. Tiap putaran, daftar simbol dibaca ulang dari `flex_params` (SIMBOL_CRYPTO yang hidup
/// + yang di-soft-delete < `KEEP_DELETED_DAYS` hari) — simbol baru otomatis ikut tanpa restart.
/// Simbol yang candle-nya sudah lengkap dilewati tanpa request ke exchange; sisanya di-fetch
/// paralel: yang belum punya data diisi histori awal, yang tertinggal dikejar dari candle
/// terakhirnya (sekaligus menambal data yang bolong).
pub async fn start(pool: PgPool) {
    loop {
        if let Err(err) = sync_all(&pool).await {
            tracing::error!("Sync candle {COLLECTOR_INTERVAL} gagal: {err:?}");
        }

        tokio::time::sleep(until_next_slot()).await;
    }
}

async fn load_symbols(pool: &PgPool) -> Result<Vec<(String, bool)>, AppError> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| AppError::Internal("Gagal memulai transaksi".to_string()))?;
    let symbols =
        find_flex_param_values_with_recently_deleted(&mut tx, SYMBOL_TYPE_PARAM, KEEP_DELETED_DAYS)
            .await?;
    let _ = tx.commit().await;

    Ok(symbols)
}

/// Kegagalan pada satu simbol (mis. pair tidak ada di exchange) tidak menghentikan simbol lain.
/// Gagal pada coin yang sudah di-soft-delete itu wajar (biasanya delisting) — tidak di-warn.
async fn sync_all(pool: &PgPool) -> Result<(), AppError> {
    let started = Instant::now();
    let symbols = load_symbols(pool).await?;
    let latest = find_latest_open_times(pool, COLLECTOR_INTERVAL).await?;
    let up_to = last_closed_open_time(chrono::Utc::now().timestamp_millis());

    let pending: Vec<(String, bool, Option<i64>)> = symbols
        .iter()
        .map(|(symbol, is_deleted)| (symbol.clone(), *is_deleted, latest.get(symbol).copied()))
        .filter(|(_, _, last)| last.is_none_or(|last| last < up_to))
        .collect();
    let skipped = symbols.len() - pending.len();
    let fetched = pending.len();

    let results: Vec<(String, bool, Result<u64, AppError>)> = stream::iter(pending)
        .map(|(symbol, is_deleted, last)| async move {
            let result = sync_symbol(pool, &symbol, last).await;
            (symbol, is_deleted, result)
        })
        .buffer_unordered(CONCURRENCY)
        .collect()
        .await;

    let mut total_inserted = 0;
    let mut failed = 0;
    for (symbol, is_deleted, result) in results {
        match result {
            Ok(inserted) => total_inserted += inserted,
            Err(err) if is_deleted => {
                tracing::debug!("Sync candle {symbol} (sudah dihapus) gagal: {err:?}");
            }
            Err(err) => {
                failed += 1;
                tracing::warn!("Sync candle {symbol} gagal: {err:?}");
            }
        }
    }

    tracing::info!(
        "Sync candle {COLLECTOR_INTERVAL} selesai dalam {:.1}s: {} simbol ({skipped} sudah lengkap, \
         {fetched} di-fetch), {total_inserted} candle baru, {failed} gagal",
        started.elapsed().as_secs_f64(),
        symbols.len()
    );
    Ok(())
}

/// Idempotent lewat `insert_candles` (ON CONFLICT DO NOTHING). Hanya candle yang sudah CLOSED
/// yang disimpan — candle hari ini yang masih berjalan sengaja diabaikan supaya dataset tidak
/// tercemar data sementara yang masih bisa berubah.
async fn sync_symbol(pool: &PgPool, symbol: &str, latest: Option<i64>) -> Result<u64, AppError> {
    let klines = match latest {
        Some(latest) => {
            binance::get_klines(symbol, COLLECTOR_INTERVAL, CATCH_UP_LIMIT, Some(latest + 1), None).await?
        }
        None => binance::get_klines(symbol, COLLECTOR_INTERVAL, INITIAL_HISTORY_LIMIT, None, None).await?,
    };

    let now_ms = chrono::Utc::now().timestamp_millis();
    let candles: Vec<NewCandle> = klines
        .iter()
        .filter(|kline| kline.close_time < now_ms)
        .filter_map(to_new_candle)
        .collect();

    insert_candles(pool, symbol, COLLECTOR_INTERVAL, &candles).await
}

/// Harga dari exchange berupa string -> `Decimal` (tanpa pembulatan float).
fn to_new_candle(kline: &binance::Kline) -> Option<NewCandle> {
    let decimal = |s: &str| Decimal::from_str(s).ok();
    Some(NewCandle {
        open_time: kline.open_time,
        open: decimal(&kline.open)?,
        high: decimal(&kline.high)?,
        low: decimal(&kline.low)?,
        close: decimal(&kline.close)?,
        volume: decimal(&kline.volume)?,
        close_time: kline.close_time,
    })
}
