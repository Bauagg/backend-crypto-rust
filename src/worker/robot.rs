use std::time::Duration;

use sqlx::PgPool;

use super::{RunFuture, RunOutcome, Schedule, WorkerDefinition};
use crate::robot::{run_demo_robot, run_live_robot, DemoRobotRun, LiveRobotRun};

/// Robot demo & live tiap hari 00:45 UTC (07:45 WIB) — setelah candle harian kemarin masuk DB
/// (worker candle mengisinya ±00:01 UTC), sesuai API_RUST.md 10.2. Data basi / ada user yang gagal
/// (mis. API Python mati) -> dicoba lagi tiap 15 menit sampai 22:00 UTC. User yang sudah diproses
/// hari ini dilewati tiap robot, jadi mengulang putaran aman — order tidak dikirim dobel.
pub const WORKER: WorkerDefinition = WorkerDefinition {
    name: "robot",
    schedule: Schedule::DailyAt {
        hour: 0,
        minute: 45,
        retry_every: Duration::from_secs(15 * 60),
        retry_until_hour: 22,
    },
    prepare: false,
    run,
};

/// Demo dulu lalu live; gagal di salah satu tidak menghentikan yang lain.
fn run(pool: PgPool) -> RunFuture {
    Box::pin(async move {
        let demo_retry = run_demo(&pool).await;
        let live_retry = run_live(&pool).await;
        Ok(if demo_retry || live_retry { RunOutcome::Retry } else { RunOutcome::Done })
    })
}

/// `true` = perlu dicoba lagi nanti.
async fn run_demo(pool: &PgPool) -> bool {
    match run_demo_robot(pool).await {
        Ok(DemoRobotRun::Done(summary)) => {
            tracing::info!(
                "[worker:robot] demo: {} user diproses, {} posisi baru, {} posisi ditutup, \
                 {} menunggu recovery, {} dimatikan (saldo kurang), {} dimatikan (kill switch), {} gagal",
                summary.processed,
                summary.positions_opened,
                summary.positions_closed,
                summary.awaiting_recovery,
                summary.deactivated_low_balance,
                summary.deactivated_kill_switch,
                summary.errors
            );
            summary.errors > 0
        }
        Ok(DemoRobotRun::StaleData { candle_date }) => {
            tracing::warn!("[worker:robot] demo ditunda: candle terakhir {candle_date}, belum kemarin (data basi)");
            true
        }
        Err(err) => {
            tracing::error!("[worker:robot] demo gagal: {err:?}");
            true
        }
    }
}

/// `true` = perlu dicoba lagi nanti. Order yang ditolak exchange TIDAK memicu coba ulang (user-nya
/// sudah ditandai diproses) — hanya user yang gagal sebelum order apa pun dikirim.
async fn run_live(pool: &PgPool) -> bool {
    match run_live_robot(pool).await {
        Ok(LiveRobotRun::Done(summary)) => {
            tracing::info!(
                "[worker:robot] live: {} user diproses, {} order terisi, {} order gagal, \
                 {} posisi baru, {} posisi ditutup, {} menunggu recovery, {} dimatikan (saldo kurang), \
                 {} dimatikan (kill switch), {} user gagal",
                summary.processed,
                summary.orders_filled,
                summary.orders_failed,
                summary.positions_opened,
                summary.positions_closed,
                summary.awaiting_recovery,
                summary.deactivated_low_balance,
                summary.deactivated_kill_switch,
                summary.errors
            );
            summary.errors > 0
        }
        Ok(LiveRobotRun::StaleData { candle_date }) => {
            tracing::warn!("[worker:robot] live ditunda: candle terakhir {candle_date}, belum kemarin (data basi)");
            true
        }
        Err(err) => {
            tracing::error!("[worker:robot] live gagal: {err:?}");
            true
        }
    }
}
