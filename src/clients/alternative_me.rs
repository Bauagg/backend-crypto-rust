//! Klien alternative.me — Fear & Greed Index (sentimen pasar crypto, 0-100).

use chrono::{DateTime, NaiveDate, Utc};
use serde::Deserialize;

use crate::utils::app_error::AppError;
use crate::utils::http_client::get_json;

fn fng_api_url() -> String {
    std::env::var("FNG_API_URL").unwrap_or_else(|_| "https://api.alternative.me/fng/".to_string())
}

#[derive(Debug, Deserialize)]
struct FngResponse {
    data: Vec<FngEntry>,
}

#[derive(Debug, Deserialize)]
struct FngEntry {
    value: String,
    timestamp: String,
}

/// Seluruh histori Fear & Greed Index yang tersedia (`limit=0`), sebagai `(tanggal, nilai)`.
pub async fn get_fng_history() -> Result<Vec<(NaiveDate, i16)>, AppError> {
    let url = format!("{}?limit=0&format=json", fng_api_url());
    let response: FngResponse = get_json(&url).await?;

    Ok(response
        .data
        .into_iter()
        .filter_map(|entry| {
            let timestamp: i64 = entry.timestamp.parse().ok()?;
            let value: i16 = entry.value.parse().ok()?;
            let date = DateTime::<Utc>::from_timestamp(timestamp, 0)?.date_naive();
            Some((date, value))
        })
        .collect())
}
