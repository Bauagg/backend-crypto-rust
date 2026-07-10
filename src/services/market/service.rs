use futures_util::future::join_all;
use serde_json::Value;
use sqlx::{PgPool, Postgres, Transaction};
use std::collections::HashMap;

use super::types::{
    is_valid_interval, Candle, KlinesQuery, MarketSymbol, OhlcvBar, ScoreLiveRequest,
    ScoreLiveResponse, Ticker24hr, TopSignal, ALLOWED_INTERVALS,
};
use crate::services::candle_market::service::get_recent_candles_asc_service;
use crate::services::fear_greed::service::get_fng_map_between_service;
use crate::services::flex_params::repository::{count_flex_params, find_all_flex_params, find_flex_params_by_type};
use crate::utils::app_error::AppError;
use crate::utils::http_client::{get_json, post_json};

const SYMBOL_TYPE_PARAM: &str = "SIMBOL_CRYPTO";
const TOP_SIGNALS_COUNT: usize = 10;
const LIVE_SIGNAL_INTERVAL: &str = "1d";
/// Minimal bar histori dikirim ke /score-live (API mensyaratkan >=60, idealnya >=200).
const LIVE_SIGNAL_MIN_BARS: usize = 60;
const LIVE_SIGNAL_LOOKBACK: i64 = 250;

fn api_base_url() -> String {
    std::env::var("MARKET_API_BASE_URL")
        .unwrap_or_else(|_| "https://www.tokocrypto.site".to_string())
}

fn strategy_api_base_url() -> String {
    std::env::var("STRATEGY_API_BASE_URL").unwrap_or_else(|_| "http://localhost:8001".to_string())
}

async fn fetch_ticker(symbol: &str) -> Option<Ticker24hr> {
    let base_url = api_base_url();
    let url = format!("{base_url}/api/v3/ticker/24hr?symbol={symbol}");
    get_json::<Ticker24hr>(&url).await.ok()
}

/// Daftar pair diambil dari `flex_params` (type_param=SIMBOL_CRYPTO) — diedit lewat endpoint
/// flex-params yang sudah ada, bukan hardcode di kode. Hanya pair yang `is_active` yang ditampilkan,
/// bisa difilter dengan search (partial match ke symbol) dan dipaginasi. Untuk tiap pair yang
/// masuk halaman ini, harga & persentase naik/turun 24 jam di-fetch on-demand dari Tokocrypto secara paralel.
pub async fn get_symbols_service(
    tx: &mut Transaction<'_, Postgres>,
    search: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<(Vec<MarketSymbol>, i64), AppError> {
    let flex_params =
        find_all_flex_params(tx, Some(SYMBOL_TYPE_PARAM), search, true, limit, offset).await?;
    let total = count_flex_params(tx, Some(SYMBOL_TYPE_PARAM), search, true).await?;

    let tickers = join_all(
        flex_params
            .iter()
            .map(|fp| fetch_ticker(&fp.value_param)),
    )
    .await;

    let symbols = flex_params
        .into_iter()
        .zip(tickers)
        .map(|(fp, ticker)| {
            let (last_price, price_change_percent) = match ticker {
                Some(t) => (t.last_price, t.price_change_percent),
                None => ("0".to_string(), "0".to_string()),
            };

            MarketSymbol {
                symbol: fp.value_param,
                photo_url: fp.photo_url,
                last_price,
                price_change_percent,
            }
        })
        .collect();

    Ok((symbols, total))
}

pub async fn get_klines_service(query: KlinesQuery) -> Result<Vec<Candle>, AppError> {
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

    let base_url = api_base_url();
    let mut url = format!(
        "{base_url}/api/v3/klines?symbol={symbol}&interval={interval}&limit={limit}"
    );
    if let Some(end_time) = query.end_time {
        url.push_str(&format!("&endTime={end_time}"));
    }

    let raw: Vec<Value> = get_json(&url).await?;

    let candles = raw
        .into_iter()
        .filter_map(|row| {
            let row = row.as_array()?;
            Some(Candle {
                open_time: row.first()?.as_i64()?,
                open: row.get(1)?.as_str()?.to_string(),
                high: row.get(2)?.as_str()?.to_string(),
                low: row.get(3)?.as_str()?.to_string(),
                close: row.get(4)?.as_str()?.to_string(),
                volume: row.get(5)?.as_str()?.to_string(),
                close_time: row.get(6)?.as_i64()?,
                is_closed: true,
            })
        })
        .collect();

    Ok(candles)
}

fn to_decimal_f64(d: rust_decimal::Decimal) -> f64 {
    d.to_string().parse().unwrap_or(0.0)
}

/// Susun body /score-live untuk 1 simbol dari histori `market_candles` kita sendiri (data live
/// Tokocrypto, bukan Binance) + histori BTC (relative strength) + histori FNG — lalu skor via API ML.
/// `None` kalau data historis simbol ini belum cukup (data baru mulai dikumpulkan collector/backfill).
async fn score_symbol_live(pool: &PgPool, symbol: &str) -> Option<ScoreLiveResponse> {
    let coin_candles = get_recent_candles_asc_service(pool, symbol, LIVE_SIGNAL_INTERVAL, LIVE_SIGNAL_LOOKBACK)
        .await
        .ok()?;
    if coin_candles.len() < LIVE_SIGNAL_MIN_BARS {
        return None;
    }

    let btc_candles = get_recent_candles_asc_service(pool, "BTCUSDT", LIVE_SIGNAL_INTERVAL, LIVE_SIGNAL_LOOKBACK)
        .await
        .ok()?;

    // hanya pakai tanggal yang tersedia di KEDUA sisi (coin & BTC) supaya index-nya sejajar
    let btc_close_by_date: HashMap<i64, f64> = btc_candles
        .iter()
        .map(|c| (c.open_time, to_decimal_f64(c.close)))
        .collect();

    let coin_candles: Vec<_> = coin_candles
        .into_iter()
        .filter(|c| btc_close_by_date.contains_key(&c.open_time))
        .collect();
    if coin_candles.len() < LIVE_SIGNAL_MIN_BARS {
        return None;
    }

    let start_date = chrono::DateTime::from_timestamp_millis(coin_candles.first()?.open_time)?.date_naive();
    let end_date = chrono::DateTime::from_timestamp_millis(coin_candles.last()?.open_time)?.date_naive();
    let fng_rows = get_fng_map_between_service(pool, start_date, end_date).await.ok()?;
    let fng_by_date: HashMap<chrono::NaiveDate, f64> = fng_rows
        .into_iter()
        .map(|(date, value)| (date, value as f64))
        .collect();

    let mut coin_ohlcv = Vec::with_capacity(coin_candles.len());
    let mut btc_close = Vec::with_capacity(coin_candles.len());
    let mut fng = Vec::with_capacity(coin_candles.len());

    for candle in &coin_candles {
        let date = chrono::DateTime::from_timestamp_millis(candle.open_time)?.date_naive();
        // FNG hari itu belum tentu ada (mis. cache belum di-refresh) -> pakai nilai netral 50
        // daripada gagalkan seluruh simbol karena satu bar tidak lengkap.
        let fng_value = fng_by_date.get(&date).copied().unwrap_or(50.0);

        coin_ohlcv.push(OhlcvBar {
            date: date.to_string(),
            open: to_decimal_f64(candle.open),
            high: to_decimal_f64(candle.high),
            low: to_decimal_f64(candle.low),
            close: to_decimal_f64(candle.close),
            volume: to_decimal_f64(candle.volume),
        });
        btc_close.push(*btc_close_by_date.get(&candle.open_time)?);
        fng.push(fng_value);
    }

    let base_url = strategy_api_base_url();
    let url = format!("{base_url}/score-live");
    let request = ScoreLiveRequest {
        symbol: symbol.to_string(),
        coin_ohlcv,
        btc_close,
        fng,
    };

    post_json::<ScoreLiveResponse>(&url, Vec::new(), &request)
        .await
        .ok()
}

/// Evaluasi SEMUA simbol aktif di `flex_params` (bukan cuma coin yang model-nya sudah dilatih
/// data historisnya) via `/score-live` — model ML dikirimi histori OHLCV mentah dari
/// `market_candles` kita sendiri (data live Tokocrypto), jadi generik untuk simbol mana pun
/// asal punya histori cukup (>=60 hari). Sinyal BUY diurutkan `ml_confidence` tertinggi, top 10.
pub async fn get_top_signals_live_service(
    pool: &PgPool,
    tx: &mut Transaction<'_, Postgres>,
) -> Result<Vec<TopSignal>, AppError> {
    let flex_params = find_flex_params_by_type(tx, SYMBOL_TYPE_PARAM, true).await?;

    let scored = join_all(
        flex_params
            .iter()
            .map(|fp| score_symbol_live(pool, &fp.value_param)),
    )
    .await;

    let photo_by_symbol: HashMap<String, Option<String>> = flex_params
        .into_iter()
        .map(|fp| (fp.value_param, fp.photo_url))
        .collect();

    let mut signals: Vec<ScoreLiveResponse> = scored
        .into_iter()
        .flatten()
        .filter(|s| s.verdict == "BUY" && s.ml_confidence.is_some())
        .collect();

    signals.sort_by(|a, b| {
        b.ml_confidence
            .unwrap_or(0.0)
            .partial_cmp(&a.ml_confidence.unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    signals.truncate(TOP_SIGNALS_COUNT);

    let top_signals = signals
        .into_iter()
        .map(|s| TopSignal {
            photo_url: photo_by_symbol.get(&s.symbol).cloned().flatten(),
            ml_confidence: s.ml_confidence.unwrap_or(0.0),
            reason: s.reason,
            close: s.close,
            ma: s.ma,
            momentum: s.momentum,
            volume: s.volume,
            relative_strength: s.relative_strength,
            reversal: s.reversal,
            regime: s.regime,
            symbol: s.symbol,
        })
        .collect();

    Ok(top_signals)
}
