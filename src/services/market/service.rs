use futures_util::future::join_all;
use serde_json::Value;
use sqlx::{Postgres, Transaction};

use super::types::{is_valid_interval, Candle, KlinesQuery, MarketSymbol, Ticker24hr, ALLOWED_INTERVALS};
use crate::services::flex_params::repository::{count_flex_params, find_all_flex_params};
use crate::utils::app_error::AppError;
use crate::utils::http_client::get_json;

const SYMBOL_TYPE_PARAM: &str = "SIMBOL_CRYPTO";

fn api_base_url() -> String {
    std::env::var("MARKET_API_BASE_URL")
        .unwrap_or_else(|_| "https://www.tokocrypto.site".to_string())
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
