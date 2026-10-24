use std::time::Duration;

use sqlx::PgPool;

use super::{RunFuture, RunOutcome, Schedule, WorkerDefinition};
use crate::services::candle_ohlcv::service::sync_candles_service;

/// Lengkapi candle harian semua simbol tiap 30 menit, disejajarkan ke jam (xx:00:30 & xx:30:30
/// UTC) — candle harian close 00:00 UTC, jadi candle kemarin sudah masuk DB ±1 menit setelahnya
/// (robot memanggil `/signal` setelah candle kemarin tersedia). Jeda 30 detik supaya exchange
/// sudah memfinalkan candle yang baru close.
pub const WORKER: WorkerDefinition = WorkerDefinition {
    name: "candle-ohlcv",
    schedule: Schedule::Aligned {
        interval: Duration::from_secs(30 * 60),
        offset: Duration::from_secs(30),
    },
    prepare: false,
    run,
};

fn run(pool: PgPool) -> RunFuture {
    Box::pin(async move {
        sync_candles_service(&pool).await?;
        Ok(RunOutcome::Done)
    })
}
