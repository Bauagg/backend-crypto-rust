//! Background worker — semua didaftarkan di satu tempat (`WORKERS`) dan dijalankan dengan cara yang
//! sama, pola seperti `router/mod.rs`. Tiap worker cukup mendefinisikan nama, jadwal, dan fungsi
//! `run` yang memanggil service; urusan jadwal, error, dan urutan start diatur di sini.
//!
//! Menambah worker baru: buat file `src/worker/<fitur>.rs` berisi `pub const WORKER`, lalu
//! tambahkan ke `WORKERS` di bawah.

mod candle_ohlcv;
mod coin_symbols;
mod fear_greed;
mod robot;
mod strategy_bot;

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use chrono::{NaiveTime, Timelike, Utc};
use sqlx::PgPool;

use crate::utils::app_error::AppError;

/// Daftar worker yang dijalankan. Urutan penting untuk worker `prepare` (dijalankan berurutan).
const WORKERS: &[WorkerDefinition] = &[
    coin_symbols::WORKER,
    candle_ohlcv::WORKER,
    fear_greed::WORKER,
    robot::WORKER,
    strategy_bot::WORKER,
];

pub type RunFuture = Pin<Box<dyn Future<Output = Result<RunOutcome, AppError>> + Send>>;

/// Hasil 1 putaran worker.
pub enum RunOutcome {
    Done,
    /// Belum tuntas (mis. data basi / layanan luar mati) — untuk jadwal `DailyAt` dicoba lagi
    /// sesuai `retry_every`. Jadwal lain cukup menunggu putaran berikutnya.
    Retry,
}

pub enum Schedule {
    /// Jalan sekali saat start, lalu tiap `Duration` setelah putaran sebelumnya selesai.
    Every(Duration),
    /// Jalan sekali saat start, lalu tiap kelipatan `interval` sejak awal hari UTC ditambah
    /// `offset` (mis. 30 menit + 30 detik = xx:00:30, xx:30:30).
    Aligned { interval: Duration, offset: Duration },
    /// Tiap hari jam `hour:minute` UTC; server start setelah jam itu = langsung jalan. Putaran yang
    /// minta `Retry` (atau error) dicoba lagi tiap `retry_every` sampai jam `retry_until_hour` UTC.
    DailyAt {
        hour: u32,
        minute: u32,
        retry_every: Duration,
        retry_until_hour: u32,
    },
}

pub struct WorkerDefinition {
    /// Nama untuk log, mis. `candle-ohlcv`.
    pub name: &'static str,
    pub schedule: Schedule,
    /// `true` = putaran pertama ditunggu selesai sebelum worker lain mulai — untuk data dasar yang
    /// dipakai worker lain (mis. daftar coin sebelum candle dikumpulkan).
    pub prepare: bool,
    pub run: fn(PgPool) -> RunFuture,
}

/// Jalankan semua worker di background — dipanggil dari `main.rs` setelah database & akun System
/// siap. Tidak menunda server menerima request.
pub fn start_workers(pool: PgPool) {
    tokio::spawn(async move {
        // Tahap 1: worker `prepare` jalan berurutan & ditunggu.
        for worker in WORKERS.iter().filter(|w| w.prepare) {
            safe_run(worker, &pool).await;
        }
        // Tahap 2: semua worker jalan sesuai jadwal masing-masing (yang `prepare` sudah jalan
        // sekali, jadi langsung menunggu jadwal berikutnya).
        for worker in WORKERS {
            tokio::spawn(run_forever(worker, pool.clone()));
        }
        let names: Vec<&str> = WORKERS.iter().map(|w| w.name).collect();
        tracing::info!("Worker aktif: {}", names.join(", "));
    });
}

/// 1 putaran yang tidak boleh menjatuhkan server: error maupun panic hanya dicatat, worker lain
/// dan putaran berikutnya tetap jalan. Error dianggap `Retry`.
async fn safe_run(worker: &WorkerDefinition, pool: &PgPool) -> RunOutcome {
    match tokio::spawn((worker.run)(pool.clone())).await {
        Ok(Ok(outcome)) => outcome,
        Ok(Err(err)) => {
            tracing::error!("[worker:{}] gagal: {err:?}", worker.name);
            RunOutcome::Retry
        }
        Err(join_err) => {
            tracing::error!("[worker:{}] panic: {join_err}", worker.name);
            RunOutcome::Retry
        }
    }
}

/// Putaran berikutnya baru dimulai setelah putaran sebelumnya selesai — tidak pernah tumpang tindih
/// walau 1 putaran lebih lama dari intervalnya.
async fn run_forever(worker: &'static WorkerDefinition, pool: PgPool) {
    match worker.schedule {
        Schedule::Every(interval) => {
            if !worker.prepare {
                safe_run(worker, &pool).await;
            }
            loop {
                tokio::time::sleep(interval).await;
                safe_run(worker, &pool).await;
            }
        }
        Schedule::Aligned { interval, offset } => {
            if !worker.prepare {
                safe_run(worker, &pool).await;
            }
            loop {
                tokio::time::sleep(until_next_slot(interval, offset)).await;
                safe_run(worker, &pool).await;
            }
        }
        Schedule::DailyAt {
            hour,
            minute,
            retry_every,
            retry_until_hour,
        } => loop {
            let scheduled = today_at(hour, minute);
            if let Ok(wait) = (scheduled - Utc::now()).to_std() {
                tokio::time::sleep(wait).await;
            }
            while let RunOutcome::Retry = safe_run(worker, &pool).await {
                if Utc::now().hour() >= retry_until_hour {
                    break;
                }
                tokio::time::sleep(retry_every).await;
            }
            let next = today_at(hour, minute) + chrono::Duration::days(1);
            tokio::time::sleep((next - Utc::now()).to_std().unwrap_or(retry_every)).await;
        },
    }
}

fn today_at(hour: u32, minute: u32) -> chrono::DateTime<Utc> {
    Utc::now()
        .date_naive()
        .and_time(NaiveTime::from_hms_opt(hour, minute, 0).expect("jam jadwal worker valid"))
        .and_utc()
}

/// Durasi sampai slot berikutnya: kelipatan `interval` sejak 00:00 UTC + `offset`.
fn until_next_slot(interval: Duration, offset: Duration) -> Duration {
    let (interval, offset) = (interval.as_secs() as i64, offset.as_secs() as i64);
    let now = Utc::now().timestamp();
    let next = (now - offset).div_euclid(interval) * interval + interval + offset;
    Duration::from_secs((next - now).max(1) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_berikutnya_tidak_lebih_dari_interval() {
        let wait = until_next_slot(Duration::from_secs(30 * 60), Duration::from_secs(30));
        assert!(wait.as_secs() >= 1 && wait.as_secs() <= 30 * 60);
    }

    #[test]
    fn nama_worker_unik() {
        let mut names: Vec<&str> = WORKERS.iter().map(|w| w.name).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), WORKERS.len());
    }
}
