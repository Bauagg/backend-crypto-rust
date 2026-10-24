use std::time::Duration;

use sqlx::PgPool;

use super::{RunFuture, RunOutcome, Schedule, WorkerDefinition};
use crate::services::coin_symbols::service::sync_coin_symbols_service;

/// Samakan daftar `SIMBOL_CRYPTO` dengan exchange (termasuk kategori & logo dari CoinGecko).
/// Listing/delisting & volume 24 jam berubah pelan — sekali sehari cukup. `prepare`: jalan duluan
/// saat start supaya coin baru langsung ikut dikumpulkan candle-nya di putaran pertama.
pub const WORKER: WorkerDefinition = WorkerDefinition {
    name: "coin-symbols",
    schedule: Schedule::Every(Duration::from_secs(24 * 60 * 60)),
    prepare: true,
    run,
};

fn run(pool: PgPool) -> RunFuture {
    Box::pin(async move {
        let summary = sync_coin_symbols_service(&pool).await?;
        tracing::info!(
            "[worker:coin-symbols] {} baru, {} dipulihkan, {} di-soft-delete, {} ganti kategori, {} foto baru",
            summary.created,
            summary.restored,
            summary.deleted,
            summary.recategorized,
            summary.photos
        );
        Ok(RunOutcome::Done)
    })
}
