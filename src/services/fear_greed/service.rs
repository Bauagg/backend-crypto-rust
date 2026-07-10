use std::time::Duration;

use chrono::NaiveDate;
use sqlx::PgPool;

use super::repository::{find_fng_between, refresh_fng_cache};
use super::types::FngPoint;
use crate::utils::app_error::AppError;

/// FNG cuma di-update 1x/hari oleh alternative.me -- 6 jam sekali cukup untuk menangkap
/// nilai hari baru tanpa polling berlebihan (tiap refresh cuma insert baris tanggal yang
/// belum ada, sisanya kena ON CONFLICT DO NOTHING sehingga bebannya sangat ringan).
const REFRESH_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

/// Fetch histori terbaru dari alternative.me lalu simpan ke cache lokal.
pub async fn refresh_fng_service(pool: &PgPool) -> Result<(), AppError> {
    refresh_fng_cache(pool).await
}

/// Jalan sebagai background task selama server hidup: refresh cache FNG secara berkala
/// supaya tidak basi walau server tidak pernah di-restart. Dipanggil terpisah dari refresh
/// awal saat startup (`refresh_fng_service` di `main.rs`) -- loop ini yang menjaganya tetap
/// mutakhir setelahnya.
pub async fn start_periodic_refresh(pool: PgPool) {
    let mut interval = tokio::time::interval(REFRESH_INTERVAL);
    interval.tick().await; // lewati tick pertama (refresh awal sudah dilakukan saat startup)

    loop {
        interval.tick().await;
        match refresh_fng_service(&pool).await {
            Ok(()) => tracing::info!("Cache Fear & Greed Index ter-refresh (job berkala)"),
            Err(err) => tracing::error!("Gagal refresh berkala Fear & Greed Index: {err:?}"),
        }
    }
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
        .map(|(date, fng_value)| FngPoint {
            date: date.to_string(),
            fng_value,
        })
        .collect())
}

/// Bentuk mentah `(tanggal, nilai)` untuk rentang tanggal — dipakai `market::service` saat
/// menyusun request `/score-live` (butuh nilai numerik sejajar per tanggal, bukan `FngPoint`).
pub async fn get_fng_map_between_service(
    pool: &PgPool,
    start: NaiveDate,
    end: NaiveDate,
) -> Result<Vec<(NaiveDate, i16)>, AppError> {
    find_fng_between(pool, start, end).await
}
