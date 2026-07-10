use sqlx::postgres::{PgPoolOptions, PgPool};

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
