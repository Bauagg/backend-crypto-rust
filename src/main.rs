mod clients;
mod config;
mod database;
mod indicator;
mod middlewares;
mod robot;
mod router;
mod services;
mod strategy_engine;
mod template;
mod utils;
mod websocket;
mod worker;

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

    // Akun System (pemilik data buatan server) harus ada sebelum worker sync jalan.
    if let Err(err) = services::users::service::ensure_system_user(&pool).await {
        tracing::error!("Gagal menyiapkan akun System: {err:?}");
    }

    // Background worker (daftar & jadwalnya di `worker/mod.rs`) — tidak menunda server siap
    // menerima request.
    worker::start_workers(pool.clone());
    // WebSocket real-time (hub chart & harga live; detailnya di `websocket/mod.rs`).
    let hubs = websocket::start(pool.clone(), redis_pool.clone());

    let upload_folder = std::env::var("PATH_FILE_UPLOAD").unwrap_or_else(|_| "files".to_string());
    let upload_folder = upload_folder.trim().to_string();

    let app = Router::new()
        .route("/", get(|| async { Json(json!({ "message": "Server is running" })) }))
        .nest_service(&format!("/{upload_folder}"), ServeDir::new(&upload_folder))
        .nest("/api", router::router())
        .with_state(pool)
        .layer(Extension(redis_pool));
    let app = hubs
        .layer(app)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string());
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}"))
        .await
        .expect("Gagal bind port");

    tracing::info!("Server running on http://localhost:{port}");
    axum::serve(listener, app).await.expect("Server error");
}
