use axum::{routing::get, Router};
use sqlx::PgPool;

use super::controller::{get_klines, get_symbols, get_top_signals_live, ws_klines};

pub fn router() -> Router<PgPool> {
    Router::new()
        .route("/klines", get(get_klines))
        .route("/symbols", get(get_symbols))
        .route("/signals/live", get(get_top_signals_live))
        .route("/ws", get(ws_klines))
}
