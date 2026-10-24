use std::time::Duration;

use sqlx::PgPool;

use super::{RunFuture, RunOutcome, Schedule, WorkerDefinition};
use crate::services::fear_greed::service::refresh_fng_service;

/// FNG cuma di-update 1x/hari oleh alternative.me — 6 jam sekali cukup untuk menangkap nilai hari
/// baru tanpa polling berlebihan (baris tanggal yang sudah ada kena ON CONFLICT DO NOTHING).
pub const WORKER: WorkerDefinition = WorkerDefinition {
    name: "fear-greed",
    schedule: Schedule::Every(Duration::from_secs(6 * 60 * 60)),
    prepare: false,
    run,
};

fn run(pool: PgPool) -> RunFuture {
    Box::pin(async move {
        refresh_fng_service(&pool).await?;
        tracing::info!("[worker:fear-greed] cache Fear & Greed Index ter-refresh");
        Ok(RunOutcome::Done)
    })
}
