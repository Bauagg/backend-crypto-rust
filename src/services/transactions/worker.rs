use std::time::Duration;

use chrono::{NaiveTime, Timelike, Utc};
use sqlx::PgPool;

use super::demo_robot::{run_demo_robot, DemoRobotRun};
use super::live_robot::{run_live_robot, LiveRobotRun};

/// Robot demo & live jalan tiap hari 00:45 UTC (07:45 WIB) — setelah candle harian kemarin masuk DB
/// (worker candle mengisinya ±00:01 UTC), sesuai API_RUST.md 10.2.
const RUN_AT_UTC: (u32, u32) = (0, 45);
/// Data basi / ada user yang gagal (mis. API Python mati) -> coba lagi setelah jeda ini.
const RETRY_DELAY: Duration = Duration::from_secs(15 * 60);
/// Batas jam (UTC) mencoba ulang di hari yang sama; setelah itu tunggu jadwal besok.
const LAST_RETRY_HOUR_UTC: u32 = 22;

fn run_time_today() -> chrono::DateTime<Utc> {
    let (hour, minute) = RUN_AT_UTC;
    Utc::now()
        .date_naive()
        .and_time(NaiveTime::from_hms_opt(hour, minute, 0).expect("jam jadwal valid"))
        .and_utc()
}

/// Jalan sebagai background task selama server hidup. Kalau server baru start setelah jam jadwal,
/// langsung diproses (user yang sudah diproses hari ini otomatis dilewati oleh tiap robot, jadi
/// mengulang putaran aman — order tidak dikirim dobel).
pub async fn start(pool: PgPool) {
    loop {
        let now = Utc::now();
        let scheduled = run_time_today();
        if now < scheduled {
            tokio::time::sleep((scheduled - now).to_std().unwrap_or_default()).await;
        }

        loop {
            let demo_retry = run_demo_once(&pool).await;
            let live_retry = run_live_once(&pool).await;
            if !(demo_retry || live_retry) || Utc::now().hour() >= LAST_RETRY_HOUR_UTC {
                break;
            }
            tokio::time::sleep(RETRY_DELAY).await;
        }

        // Tunggu jadwal besok.
        let next = run_time_today() + chrono::Duration::days(1);
        tokio::time::sleep((next - Utc::now()).to_std().unwrap_or(RETRY_DELAY)).await;
    }
}

/// `true` = perlu dicoba lagi nanti.
async fn run_demo_once(pool: &PgPool) -> bool {
    match run_demo_robot(pool).await {
        Ok(DemoRobotRun::Done(summary)) => {
            tracing::info!(
                "Robot demo selesai: {} user diproses, {} posisi baru, {} posisi ditutup, \
                 {} dimatikan (saldo kurang), {} dimatikan (kill switch), {} gagal",
                summary.processed,
                summary.positions_opened,
                summary.positions_closed,
                summary.deactivated_low_balance,
                summary.deactivated_kill_switch,
                summary.errors
            );
            summary.errors > 0
        }
        Ok(DemoRobotRun::StaleData { candle_date }) => {
            tracing::warn!("Robot demo ditunda: candle terakhir {candle_date}, belum kemarin (data basi)");
            true
        }
        Err(err) => {
            tracing::error!("Robot demo gagal: {err:?}");
            true
        }
    }
}

/// `true` = perlu dicoba lagi nanti. Order yang ditolak exchange TIDAK memicu coba ulang (user-nya
/// sudah ditandai diproses) — hanya user yang gagal sebelum order apa pun dikirim.
async fn run_live_once(pool: &PgPool) -> bool {
    match run_live_robot(pool).await {
        Ok(LiveRobotRun::Done(summary)) => {
            tracing::info!(
                "Robot live selesai: {} user diproses, {} order terisi, {} order gagal, \
                 {} posisi baru, {} posisi ditutup, {} dimatikan (saldo kurang), \
                 {} dimatikan (kill switch), {} user gagal",
                summary.processed,
                summary.orders_filled,
                summary.orders_failed,
                summary.positions_opened,
                summary.positions_closed,
                summary.deactivated_low_balance,
                summary.deactivated_kill_switch,
                summary.errors
            );
            summary.errors > 0
        }
        Ok(LiveRobotRun::StaleData { candle_date }) => {
            tracing::warn!("Robot live ditunda: candle terakhir {candle_date}, belum kemarin (data basi)");
            true
        }
        Err(err) => {
            tracing::error!("Robot live gagal: {err:?}");
            true
        }
    }
}
