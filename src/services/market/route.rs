use axum::{routing::get, Router};
use sqlx::PgPool;

use super::controller::{get_klines, get_recommendations, get_symbols, ws_klines, ws_tickers};

pub fn router() -> Router<PgPool> {
    Router::new()
        .route("/klines", get(get_klines))
        .route("/symbols", get(get_symbols))
        .route("/recommendations", get(get_recommendations))
        .route("/ws", get(ws_klines))
        .route("/ws/tickers", get(ws_tickers))
}
