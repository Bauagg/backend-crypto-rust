mod clients;
mod config;
mod database;
mod middlewares;
mod router;
mod services;
mod utils;

use axum::{routing::get, Extension, Json, Router};
use serde_json::json;
use tower_http::{cors::CorsLayer, services::ServeDir, trace::TraceLayer};

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    let _log_guard = config::logger::init();

    // Secret dibaca & divalidasi sekali di sini: env kosong = server gagal start dengan pesan jelas.
    utils::jwt::init().expect("Konfigurasi JWT tidak valid");
    utils::crypto::init().expect("Konfigurasi ENCRYPTION_KEY tidak valid");

    let pool = database::connect().await;
    let redis_pool = database::connect_redis().await;
    let kline_hub = services::market::stream_hub::KlineHub::new(redis_pool.clone());
    let ticker_hub = services::market::ticker_hub::TickerHub::new();

    // Akun System (pemilik data buatan server) harus ada sebelum worker sync jalan.
    if let Err(err) = services::users::service::ensure_system_user(&pool).await {
        tracing::error!("Gagal menyiapkan akun System: {err:?}");
    }

    // Background worker: jalan terus selama server hidup, tidak menunda server siap menerima request.
    tokio::spawn(start_market_workers(pool.clone()));
    tokio::spawn(ticker_hub.clone().run(pool.clone()));
    tokio::spawn(services::fear_greed::worker::start(pool.clone()));
    tokio::spawn(services::transactions::worker::start(pool.clone()));

    let upload_folder = std::env::var("PATH_FILE_UPLOAD").unwrap_or_else(|_| "files".to_string());
    let upload_folder = upload_folder.trim().to_string();

    let app = Router::new()
        .route("/", get(|| async { Json(json!({ "message": "Server is running" })) }))
        .nest_service(&format!("/{upload_folder}"), ServeDir::new(&upload_folder))
        .nest("/api", router::router())
        .with_state(pool)
        .layer(Extension(redis_pool))
        .layer(Extension(kline_hub))
        .layer(Extension(ticker_hub))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string());
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}"))
        .await
        .expect("Gagal bind port");

    tracing::info!("Server running on http://localhost:{port}");
    axum::serve(listener, app).await.expect("Server error");
}

/// Daftar coin disinkronkan dulu sebelum worker candle jalan — supaya coin baru hasil sync
/// langsung ikut dikumpulkan candle-nya di putaran pertama, bukan menunggu 30 menit.
async fn start_market_workers(pool: sqlx::PgPool) {
    services::coin_symbols::worker::sync_once(&pool).await;
    tokio::spawn(services::candle_ohlcv::worker::start(pool.clone()));
    services::coin_symbols::worker::start(pool).await;
}
