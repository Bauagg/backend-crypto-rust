use chrono::NaiveDate;
use sqlx::PgPool;

use super::model::FearGreedIndex;
use crate::utils::app_error::AppError;

/// Insert histori FNG yang belum ada ke cache lokal (idempotent, tidak menimpa yang sudah ada
/// karena nilai historis FNG tidak pernah berubah setelah publish).
pub async fn insert_fng_entries(pool: &PgPool, entries: &[(NaiveDate, i16)]) -> Result<(), AppError> {
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

/// Ambil histori FNG untuk rentang tanggal tertentu, terurut lama->baru. Baca dari cache lokal
/// saja — panggil `refresh_fng_cache` secara terpisah untuk mengisi/memperbarui cache.
pub async fn find_fng_between(
    pool: &PgPool,
    start: NaiveDate,
    end: NaiveDate,
) -> Result<Vec<FearGreedIndex>, AppError> {
    let rows = sqlx::query_as::<_, FearGreedIndex>(
        "SELECT * FROM fear_greed_index WHERE date >= $1 AND date <= $2 ORDER BY date ASC",
    )
    .bind(start)
    .bind(end)
    .fetch_all(pool)
    .await?;

    Ok(rows)
}
