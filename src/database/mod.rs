use deadpool_redis::{Config as RedisConfig, Runtime};
use sqlx::postgres::{PgPoolOptions, PgPool};
use sqlx::{Postgres, Transaction};

use crate::utils::app_error::AppError;

/// Alias supaya modul lain yang butuh pool Redis (lewat `Extension<RedisPool>`) tidak perlu
/// import `deadpool_redis` langsung.
pub type RedisPool = deadpool_redis::Pool;

/// Transaksi DB Postgres yang dipinjamkan service ke repository.
pub type Tx<'a> = Transaction<'a, Postgres>;

/// Mulai transaksi DB. Dipanggil di service (bukan controller); transaksi yang tidak di-`commit`
/// (mis. keluar lebih awal lewat `?`) otomatis di-rollback saat di-drop.
pub async fn begin(pool: &PgPool) -> Result<Tx<'_>, AppError> {
    pool.begin()
        .await
        .map_err(|_| AppError::Internal("Gagal memulai transaksi".to_string()))
}

pub async fn commit(tx: Tx<'_>) -> Result<(), AppError> {
    tx.commit()
        .await
        .map_err(|_| AppError::Internal("Gagal menyimpan perubahan".to_string()))
}

pub async fn connect() -> PgPool {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL tidak ditemukan di .env");

    // Pool penuh -> request gagal cepat (503-ish) setelah 5 detik, tidak menggantung 30 detik.
    let pool = PgPoolOptions::new()
        .max_connections(10)
        .acquire_timeout(std::time::Duration::from_secs(5))
        .connect(&url)
        .await
        .expect("Gagal konek ke database");

    tracing::info!("Database connected successfully");

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Gagal menjalankan migration");

    tracing::info!("Database synced successfully");

    pool
}

pub async fn connect_redis() -> RedisPool {
    let url = std::env::var("REDIS_URL").expect("REDIS_URL tidak ditemukan di .env");

    let pool = RedisConfig::from_url(url)
        .create_pool(Some(Runtime::Tokio1))
        .expect("Gagal membuat pool Redis");

    // Pastikan koneksi benar-benar bisa dibuat saat startup, bukan baru gagal nanti saat dipakai.
    pool.get().await.expect("Gagal konek ke Redis");

    tracing::info!("Redis connected successfully");

    pool
}
