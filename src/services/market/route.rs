use axum::{routing::get, Router};
use sqlx::PgPool;

use super::controller::{get_klines, get_stored_candles, get_symbols, ws_klines};

pub fn router() -> Router<PgPool> {
    Router::new()
        .route("/klines", get(get_klines))
        .route("/candles", get(get_stored_candles))
        .route("/symbols", get(get_symbols))
        .route("/ws", get(ws_klines))
}
