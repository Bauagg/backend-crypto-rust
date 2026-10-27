use std::collections::HashMap;

use chrono::{Duration, NaiveDate, Utc};
use sqlx::PgPool;

use super::repository::{find_fng_between, insert_fng_entries};
use super::types::FngPoint;
use crate::clients::alternative_me;
use crate::utils::app_error::AppError;

/// Ambil seluruh histori FNG dari alternative.me lalu simpan yang belum ada ke tabel lokal,
/// supaya pembacaan berikutnya tidak perlu ke API luar.
pub async fn refresh_fng_service(pool: &PgPool) -> Result<(), AppError> {
    let entries = alternative_me::get_fng_history().await?;
    insert_fng_entries(pool, &entries).await
}

/// Nilai Fear & Greed `days` hari terakhir (lama -> baru) dari cache lokal — bahan screener &
/// strategi buatan user. Kosong kalau belum ada data.
pub async fn recent_fear_greed_values(pool: &PgPool, days: i64) -> Result<Vec<f64>, AppError> {
    let today = Utc::now().date_naive();
    Ok(find_fng_between(pool, today - Duration::days(days), today)
        .await?
        .into_iter()
        .map(|row| f64::from(row.fng_value))
        .collect())
}

/// Nilai Fear & Greed per tanggal `days` hari terakhir — bahan backtest strategi.
pub async fn fear_greed_by_date(pool: &PgPool, days: i64) -> Result<HashMap<NaiveDate, f64>, AppError> {
    let today = Utc::now().date_naive();
    Ok(find_fng_between(pool, today - Duration::days(days), today)
        .await?
        .into_iter()
        .map(|row| (row.date, f64::from(row.fng_value)))
        .collect())
}

/// Ambil histori FNG (dari cache lokal) untuk rentang tanggal — dipakai untuk diekspos ke FE.
pub async fn get_fng_between_service(
    pool: &PgPool,
    start: NaiveDate,
    end: NaiveDate,
) -> Result<Vec<FngPoint>, AppError> {
    let rows = find_fng_between(pool, start, end).await?;
    Ok(rows
        .into_iter()
        .map(|row| FngPoint {
            date: row.date.to_string(),
            fng_value: row.fng_value,
        })
        .collect())
}
