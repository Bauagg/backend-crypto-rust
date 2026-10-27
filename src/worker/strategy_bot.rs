use std::time::Duration;

use sqlx::PgPool;

use super::{RunFuture, RunOutcome, Schedule, WorkerDefinition};
use crate::robot::{run_strategy_bots, StrategyBotRun};

/// Robot strategi buatan user tiap hari 01:00 UTC (08:00 WIB) — setelah candle harian kemarin
/// masuk (±00:01 UTC) dan setelah robot Python (00:45 UTC), supaya keduanya tidak memakai saldo
/// bersamaan. Data basi / ada bot yang gagal -> dicoba lagi tiap 15 menit sampai 22:00 UTC. Bot
/// yang sudah diproses hari ini dilewati, jadi mengulang putaran aman — order tidak dikirim dobel.
pub const WORKER: WorkerDefinition = WorkerDefinition {
    name: "strategy-bot",
    schedule: Schedule::DailyAt {
        hour: 1,
        minute: 0,
        retry_every: Duration::from_secs(15 * 60),
        retry_until_hour: 22,
    },
    prepare: false,
    run,
};

fn run(pool: PgPool) -> RunFuture {
    Box::pin(async move {
        let retry = match run_strategy_bots(&pool).await {
            Ok(StrategyBotRun::Done(summary)) => {
                tracing::info!(
                    "[worker:strategy-bot] {} bot diproses, {} dilewati, {} order terisi, {} order gagal, \
                     {} posisi baru, {} posisi ditutup, {} bot gagal",
                    summary.processed,
                    summary.skipped,
                    summary.orders_filled,
                    summary.orders_failed,
                    summary.positions_opened,
                    summary.positions_closed,
                    summary.errors
                );
                summary.errors > 0
            }
            Ok(StrategyBotRun::StaleData { candle_date }) => {
                tracing::warn!("[worker:strategy-bot] ditunda: candle terakhir {candle_date:?}, belum kemarin (data basi)");
                true
            }
            Err(err) => {
                tracing::error!("[worker:strategy-bot] gagal: {err:?}");
                true
            }
        };
        Ok(if retry { RunOutcome::Retry } else { RunOutcome::Done })
    })
}
