use chrono::{DateTime, NaiveDate, Utc};
use sqlx::PgPool;

use super::types::RawFngResponse;
use crate::utils::app_error::AppError;
use crate::utils::http_client::get_json;

fn fng_api_url() -> String {
    std::env::var("FNG_API_URL").unwrap_or_else(|_| "https://api.alternative.me/fng/".to_string())
}

/// Insert histori FNG yang belum ada ke cache lokal (idempotent, tidak menimpa yang sudah ada
/// karena nilai historis FNG tidak pernah berubah setelah publish).
async fn cache_fng_entries(pool: &PgPool, entries: &[(NaiveDate, i16)]) -> Result<(), AppError> {
    for (date, value) in entries {
        sqlx::query(
            "INSERT INTO fear_greed_index (date, fng_value) VALUES ($1, $2) ON CONFLICT (date) DO NOTHING",
        )
        .bind(date)
        .bind(value)
        .execute(pool)
        .await?;
    }
    Ok(())
}

/// Fetch histori FNG langsung dari alternative.me (limit=0 -> semua histori yang tersedia),
/// lalu cache ke tabel lokal supaya request berikutnya tidak perlu fetch API eksternal lagi.
pub async fn refresh_fng_cache(pool: &PgPool) -> Result<(), AppError> {
    let url = format!("{}?limit=0&format=json", fng_api_url());
    let response: RawFngResponse = get_json(&url).await?;

    let entries: Vec<(NaiveDate, i16)> = response
        .data
        .into_iter()
        .filter_map(|entry| {
            let timestamp: i64 = entry.timestamp.parse().ok()?;
            let value: i16 = entry.value.parse().ok()?;
            let date = DateTime::<Utc>::from_timestamp(timestamp, 0)?.date_naive();
            Some((date, value))
        })
        .collect();

    cache_fng_entries(pool, &entries).await
}

/// Ambil histori FNG untuk rentang tanggal tertentu, terurut lama->baru (sesuai urutan yang
/// dibutuhkan `coin_ohlcv`/`btc_close` saat dikirim ke API ML). Baca dari cache lokal saja —
/// panggil `refresh_fng_cache` secara terpisah untuk mengisi/memperbarui cache.
pub async fn find_fng_between(
    pool: &PgPool,
    start: NaiveDate,
    end: NaiveDate,
) -> Result<Vec<(NaiveDate, i16)>, AppError> {
    let rows: Vec<(NaiveDate, i16)> = sqlx::query_as(
        "SELECT date, fng_value FROM fear_greed_index WHERE date >= $1 AND date <= $2 ORDER BY date ASC",
    )
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await?;

    Ok(rows)
}
