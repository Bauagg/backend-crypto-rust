use deadpool_redis::{Config as RedisConfig, Runtime};
use sqlx::postgres::{PgPoolOptions, PgPool};

/// Alias supaya modul lain yang butuh pool Redis (lewat `Extension<RedisPool>`) tidak perlu
/// import `deadpool_redis` langsung.
pub type RedisPool = deadpool_redis::Pool;

pub async fn connect() -> PgPool {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL tidak ditemukan di .env");

    let pool = PgPoolOptions::new()
        .max_connections(10)
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
