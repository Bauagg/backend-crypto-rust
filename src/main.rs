mod config;
mod database;
mod middlewares;
mod router;
mod services;
mod utils;

use axum::{routing::get, Json, Router};
use serde_json::json;
use tower_http::{cors::CorsLayer, services::ServeDir, trace::TraceLayer};

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    let _log_guard = config::logger::init();

    let pool = database::connect().await;

    tokio::spawn(services::market::collector::start(pool.clone()));

    let upload_folder = std::env::var("PATH_FILE_UPLOAD").unwrap_or_else(|_| "files".to_string());
    let upload_folder = upload_folder.trim().to_string();

    let app = Router::new()
        .route("/", get(|| async { Json(json!({ "message": "Server is running" })) }))
        .nest_service(&format!("/{upload_folder}"), ServeDir::new(&upload_folder))
        .nest("/api", router::router())
        .with_state(pool)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string());
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}"))
        .await
        .expect("Gagal bind port");

    tracing::info!("Server running on http://localhost:{port}");
    axum::serve(listener, app).await.expect("Server error");
}
