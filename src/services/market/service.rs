use chrono::Timelike;
use deadpool_redis::redis::AsyncCommands;
use serde_json::Value;
use sqlx::PgPool;
use std::collections::HashMap;

use super::repository;
use super::ticker_hub::TickerHub;
use super::types::{
    is_valid_interval, Candle, KlinesQuery, MarketSymbol, MarketTicker, RecommendationQuery,
    ALLOWED_INTERVALS,
};
use crate::clients::{binance, strategy_api};
use crate::database::RedisPool;
use crate::services::candle_ohlcv::repository::find_last_two_closes;
use crate::services::flex_params::repository::{count_flex_params, find_all_flex_params};
use crate::utils::app_error::AppError;

const SYMBOL_TYPE_PARAM: &str = "SIMBOL_CRYPTO";
/// Cache singkat REST /klines di Redis — bukan pengganti WebSocket (yang tetap real-time),
/// cuma meredam lonjakan request bersamaan ke exchange saat banyak client buka chart simbol
/// yang sama dalam waktu berdekatan (mis. reload halaman FE beruntun).
const KLINES_CACHE_TTL_SECONDS: u64 = 5;

/// Ticker 24hr hanya untuk simbol yang diminta, dalam 1 request — bukan ticker semua pair
/// exchange (±3.500 pair, beberapa MB, sering timeout lewat koneksi lambat). `None` kalau
/// request gagal (mis. rate limit 429); pemanggil fallback ke candle tersimpan.
async fn fetch_page_tickers(symbols: &[String]) -> Option<HashMap<String, binance::Ticker24hr>> {
    let tickers = match binance::get_tickers_24hr_for(symbols).await {
        Ok(t) => t,
        Err(err) => {
            tracing::warn!("Gagal mengambil ticker 24hr dari exchange: {err:?}");
            return None;
        }
    };

    Some(tickers.into_iter().map(|t| (t.symbol.clone(), t)).collect())
}

/// `last_price` & `price_change_percent` dari 2 candle harian terakhir yang kita simpan sendiri
/// di `candle_ohlcv` — fallback kalau harga exchange gagal didapat, supaya tidak langsung jatuh ke
/// "0" selama masih punya histori candle. Semua simbol diambil dalam 1 query.
async fn fallback_prices_from_candles(
    pool: &PgPool,
    symbols: &[String],
) -> HashMap<String, (String, String)> {
    let closes = match find_last_two_closes(pool, symbols, "1d").await {
        Ok(closes) => closes,
        Err(err) => {
            tracing::warn!("Fallback harga dari candle gagal: {err:?}");
            return HashMap::new();
        }
    };
    closes
        .into_iter()
        .map(|(symbol, (last, prev))| {
            let change = match prev {
                Some(prev) if !prev.is_zero() => (last - prev) / prev * rust_decimal::Decimal::ONE_HUNDRED,
                _ => rust_decimal::Decimal::ZERO,
            };
            (symbol, (last.to_string(), change.round_dp(2).to_string()))
        })
        .collect()
}

/// Daftar pair diambil dari `flex_params` (type_param=SIMBOL_CRYPTO) — diedit lewat endpoint
/// flex-params yang sudah ada, bukan hardcode di kode. Hanya pair yang `is_active` yang ditampilkan,
/// bisa difilter dengan search (partial match ke symbol) dan dipaginasi. Harga & persentase naik/turun
/// 24 jam dibaca dari memori `TickerHub` (live, tanpa request ke exchange). Simbol yang belum ada di
/// sana (mis. server baru start) diambil lewat 1 fetch REST untuk simbol itu saja; kalau gagal juga,
/// fallback ke candle harian tersimpan sendiri (bisa basi s/d 1 hari, tapi lebih berguna daripada "0").
///
/// Transaksi DB hanya untuk membaca daftar coin, dan sudah selesai sebelum harga diambil — koneksi
/// DB tidak tertahan selama menunggu exchange.
pub async fn get_symbols_service(
    pool: &PgPool,
    ticker_hub: &TickerHub,
    search: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<(Vec<MarketSymbol>, i64), AppError> {
    let (flex_params, total) = {
        let mut tx = pool
            .begin()
            .await
            .map_err(|_| AppError::Internal("Gagal memulai transaksi".to_string()))?;
        let flex_params =
            find_all_flex_params(&mut tx, Some(SYMBOL_TYPE_PARAM), search, true, limit, offset).await?;
        let total = count_flex_params(&mut tx, Some(SYMBOL_TYPE_PARAM), search, true).await?;
        let _ = tx.commit().await;
        (flex_params, total)
    };

    let page_symbols: Vec<String> = flex_params.iter().map(|fp| fp.value_param.clone()).collect();
    let mut tickers = ticker_hub.get(&page_symbols);
    let missing: Vec<String> =
        page_symbols.into_iter().filter(|s| !tickers.contains_key(s)).collect();
    if !missing.is_empty() {
        for (symbol, t) in fetch_page_tickers(&missing).await.unwrap_or_default() {
            let ticker = MarketTicker {
                symbol: symbol.clone(),
                last_price: t.last_price,
                price_change_percent: t.price_change_percent,
            };
            tickers.insert(symbol, ticker);
        }
    }

    let still_missing: Vec<String> = flex_params
        .iter()
        .map(|fp| fp.value_param.clone())
        .filter(|s| !tickers.contains_key(s))
        .collect();
    let fallback = fallback_prices_from_candles(pool, &still_missing).await;

    let mut symbols = Vec::with_capacity(flex_params.len());
    for fp in flex_params {
        let (last_price, price_change_percent) = match tickers.get(&fp.value_param) {
            Some(t) => (t.last_price.clone(), t.price_change_percent.clone()),
            None => fallback
                .get(&fp.value_param)
                .cloned()
                .unwrap_or_else(|| ("0".to_string(), "0".to_string())),
        };

        symbols.push(MarketSymbol {
            symbol: fp.value_param,
            photo_url: fp.photo_url,
            last_price,
            price_change_percent,
        });
    }

    Ok((symbols, total))
}

/// Key cache Redis unik per kombinasi parameter — beda parameter harus beda hasil, tidak boleh
/// saling menimpa cache satu sama lain.
fn klines_cache_key(symbol: &str, interval: &str, limit: u16, end_time: Option<i64>) -> String {
    format!(
        "klines:{symbol}:{interval}:{limit}:{}",
        end_time.map(|t| t.to_string()).unwrap_or_default()
    )
}

async fn fetch_klines(
    symbol: &str,
    interval: &str,
    limit: u16,
    end_time: Option<i64>,
) -> Result<Vec<Candle>, AppError> {
    let klines = binance::get_klines(symbol, interval, limit, None, end_time).await?;
    Ok(klines.into_iter().map(Candle::from).collect())
}

/// Data chart. Halaman histori (`end_time` diisi) dilayani dari cache rentang (`repository`) yang
/// dipakai bersama semua user & semua panjang rentang. Halaman terbaru diambil dari Redis kalau ada
/// stream live aktif (`stream_hub`); kalau tidak, dari exchange dengan cache per request 5 detik —
/// candle closed-nya tetap ikut mengisi cache rentang.
pub async fn get_klines_service(
    redis: &RedisPool,
    query: KlinesQuery,
) -> Result<Vec<Candle>, AppError> {
    if query.symbol.trim().is_empty() {
        return Err(AppError::BadRequest("symbol wajib diisi".to_string()));
    }

    let interval = query.interval.unwrap_or_else(|| "1h".to_string());
    if !is_valid_interval(&interval) {
        return Err(AppError::BadRequest(format!(
            "interval tidak valid, pilihan: {}",
            ALLOWED_INTERVALS.join(", ")
        )));
    }

    let limit = query.limit.unwrap_or(500).clamp(1, 1000);
    let symbol = query.symbol.trim().to_uppercase();

    match query.end_time {
        Some(end_time) if repository::interval_ms(&interval).is_some() => {
            get_history_klines(redis, &symbol, &interval, limit, end_time).await
        }
        end_time => get_latest_klines(redis, &symbol, &interval, limit, end_time).await,
    }
}

async fn get_history_klines(
    redis: &RedisPool,
    symbol: &str,
    interval: &str,
    limit: u16,
    end_time: i64,
) -> Result<Vec<Candle>, AppError> {
    if let Some(candles) = repository::read_range(redis, symbol, interval, end_time, limit).await {
        return Ok(candles);
    }

    // Banyak user minta rentang yang sama bersamaan -> cuma 1 yang ke exchange, sisanya menunggu.
    let locked = repository::try_lock(redis, symbol, interval, end_time, limit).await;
    if !locked
        && let Some(candles) =
            repository::wait_for_range(redis, symbol, interval, end_time, limit).await
        {
            return Ok(candles);
        }

    let result = fetch_klines(symbol, interval, limit, Some(end_time)).await;
    if let Ok(candles) = &result {
        let reached_listing_start = candles.len() < limit as usize;
        repository::store_closed(redis, symbol, interval, candles, reached_listing_start).await;
    }
    if locked {
        repository::unlock(redis, symbol, interval, end_time, limit).await;
    }
    result
}

async fn get_latest_klines(
    redis: &RedisPool,
    symbol: &str,
    interval: &str,
    limit: u16,
    end_time: Option<i64>,
) -> Result<Vec<Candle>, AppError> {
    // Ada client yang sedang membuka chart ini -> stream hub menjaga candle live & histori di
    // Redis tetap terbaru, jadi tidak perlu ke exchange sama sekali.
    if end_time.is_none()
        && let Some(candles) = repository::read_latest(redis, symbol, interval, limit).await {
            return Ok(candles);
        }

    let cache_key = klines_cache_key(symbol, interval, limit, end_time);

    if let Ok(mut conn) = redis.get().await
        && let Ok(Some(cached)) = conn.get::<_, Option<String>>(&cache_key).await
            && let Ok(candles) = serde_json::from_str::<Vec<Candle>>(&cached) {
                return Ok(candles);
            }

    let candles = fetch_klines(symbol, interval, limit, end_time).await?;

    // Cache best-effort: gagal simpan (Redis down, dll) tidak boleh menggagalkan response.
    if let Ok(mut conn) = redis.get().await
        && let Ok(serialized) = serde_json::to_string(&candles) {
            let _: Result<(), _> = conn
                .set_ex(&cache_key, serialized, KLINES_CACHE_TTL_SECONDS)
                .await;
        }
    let reached_listing_start = candles.len() < limit as usize;
    repository::store_closed(redis, symbol, interval, &candles, reached_listing_start).await;

    Ok(candles)
}


const MIN_RECOMMENDATION_LIMIT: u8 = 5;
const MAX_RECOMMENDATION_LIMIT: u8 = 10;
/// Rekomendasi cuma berubah sekali sehari (setelah candle harian close 00:00 UTC), tapi cache
/// dibatasi maksimal 1 jam supaya kalau candle telat masuk, rekomendasi basi tidak bertahan lama.
const RECOMMENDATION_MAX_CACHE_SECONDS: u64 = 60 * 60;
/// Rekomendasi tanggal tertentu (histori) tidak akan berubah lagi.
const RECOMMENDATION_PAST_DATE_CACHE_SECONDS: u64 = 24 * 60 * 60;
/// Candle harian sudah masuk DB ±00:01 UTC (worker candle) — cache rekomendasi "terbaru" dibuang
/// paling lambat jam segini supaya hari baru langsung dihitung ulang.
const RECOMMENDATION_REFRESH_UTC_MINUTE: u32 = 5;

fn recommendation_cache_key(limit: u8, date: Option<&str>) -> String {
    format!("recommendations:momentum:{limit}:{}", date.unwrap_or("latest"))
}

/// Detik sampai 00:05 UTC berikutnya, maksimal `RECOMMENDATION_MAX_CACHE_SECONDS`.
fn latest_recommendation_cache_seconds() -> u64 {
    let seconds_today = chrono::Utc::now().num_seconds_from_midnight() as i64;
    let refresh_at = (RECOMMENDATION_REFRESH_UTC_MINUTE * 60) as i64;
    let until_refresh = if seconds_today < refresh_at {
        refresh_at - seconds_today
    } else {
        24 * 60 * 60 - seconds_today + refresh_at
    };
    (until_refresh.max(1) as u64).min(RECOMMENDATION_MAX_CACHE_SECONDS)
}

/// Rekomendasi momentum (daftar pantauan coin, bukan sinyal bot) dari API strategi Python,
/// diteruskan apa adanya — termasuk `id` & `image_url` tiap coin yang sudah disiapkan Python dari
/// `flex_params`. Di-cache di Redis: isinya cuma berubah sekali sehari, jadi Python tidak perlu
/// dipanggil tiap kali user membuka halaman.
pub async fn get_recommendations_service(
    redis: &RedisPool,
    query: RecommendationQuery,
) -> Result<Value, AppError> {
    let limit = query.limit.unwrap_or(MAX_RECOMMENDATION_LIMIT);
    if !(MIN_RECOMMENDATION_LIMIT..=MAX_RECOMMENDATION_LIMIT).contains(&limit) {
        return Err(AppError::BadRequest(format!(
            "limit harus {MIN_RECOMMENDATION_LIMIT}-{MAX_RECOMMENDATION_LIMIT}"
        )));
    }

    let date = query.date.as_deref().map(str::trim).filter(|d| !d.is_empty());
    if let Some(date) = date {
        chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
            .map_err(|_| AppError::BadRequest("date harus format YYYY-MM-DD".to_string()))?;
    }

    let key = recommendation_cache_key(limit, date);
    if let Ok(mut conn) = redis.get().await
        && let Ok(Some(cached)) = conn.get::<_, Option<String>>(&key).await
            && let Ok(value) = serde_json::from_str::<Value>(&cached) {
                return Ok(value);
            }

    let response = strategy_api::get_momentum_recommendations(limit, date).await?;

    // Cache best-effort: Redis bermasalah tidak boleh menggagalkan response.
    let ttl = if date.is_some() {
        RECOMMENDATION_PAST_DATE_CACHE_SECONDS
    } else {
        latest_recommendation_cache_seconds()
    };
    if let (Ok(mut conn), Ok(serialized)) = (redis.get().await, serde_json::to_string(&response)) {
        let _: Result<(), _> = conn.set_ex(&key, serialized, ttl).await;
    }

    Ok(response)
}
