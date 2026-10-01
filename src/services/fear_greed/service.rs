use chrono::NaiveDate;
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
