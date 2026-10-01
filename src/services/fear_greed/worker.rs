use std::time::Duration;

use sqlx::PgPool;

use super::service::refresh_fng_service;

/// FNG cuma di-update 1x/hari oleh alternative.me -- 6 jam sekali cukup untuk menangkap
/// nilai hari baru tanpa polling berlebihan (tiap refresh cuma insert baris tanggal yang
/// belum ada, sisanya kena ON CONFLICT DO NOTHING sehingga bebannya sangat ringan).
const REFRESH_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

/// Jalan sebagai background task selama server hidup: refresh cache FNG langsung saat startup,
/// lalu berkala tiap `REFRESH_INTERVAL` supaya tidak basi walau server tidak pernah di-restart.
pub async fn start(pool: PgPool) {
    let mut interval = tokio::time::interval(REFRESH_INTERVAL);

    loop {
        interval.tick().await;
        match refresh_fng_service(&pool).await {
            Ok(()) => tracing::info!("Cache Fear & Greed Index ter-refresh"),
            Err(err) => tracing::error!("Gagal refresh Fear & Greed Index: {err:?}"),
        }
    }
}
