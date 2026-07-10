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

    tokio::spawn(startup_market_data(pool.clone()));
    tokio::spawn(services::candle_market::collector::start(pool.clone()));
    tokio::spawn(services::fear_greed::service::start_periodic_refresh(pool.clone()));

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

/// Sekali jalan saat startup (di background, tidak menunda server siap menerima request):
/// isi histori `market_candles` untuk semua simbol aktif dari REST Tokocrypto (`backfill`),
/// dan refresh cache Fear & Greed Index — keduanya dibutuhkan endpoint `/api/market/signals/live`.
async fn startup_market_data(pool: sqlx::PgPool) {
    if let Err(err) = services::fear_greed::service::refresh_fng_service(&pool).await {
        tracing::error!("Gagal refresh cache Fear & Greed Index: {err:?}");
    } else {
        tracing::info!("Cache Fear & Greed Index berhasil di-refresh");
    }

    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(err) => {
            tracing::error!("Gagal memulai transaksi untuk backfill: {err}");
            return;
        }
    };

    let flex_params = match services::flex_params::repository::find_flex_params_by_type(
        &mut tx,
        "SIMBOL_CRYPTO",
        true,
    )
    .await
    {
        Ok(params) => params,
        Err(err) => {
            tracing::error!("Gagal mengambil daftar simbol untuk backfill: {err:?}");
            return;
        }
    };
    let _ = tx.commit().await;

    let symbols: Vec<String> = flex_params.into_iter().map(|fp| fp.value_param).collect();
    services::candle_market::backfill::backfill_symbols(&pool, &symbols).await;
    tracing::info!("Backfill histori market_candles selesai untuk {} simbol", symbols.len());
}
