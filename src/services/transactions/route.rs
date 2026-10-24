use axum::{middleware, routing::get, Router};
use sqlx::PgPool;

use super::controller::{
    create_trade_position, get_dashboard, get_holding, get_holding_by_symbol,
    get_trade_position_by_id, get_trade_positions,
};
use crate::middlewares::authenticate::authenticate;

pub fn router() -> Router<PgPool> {
    Router::new()
        // POST "/" = BUY & SELL manual (`transaction_type`).
        .route("/", get(get_trade_positions).post(create_trade_position))
        .route("/dashboard", get(get_dashboard))
        // Kepemilikan 1 coin (total + rincian posisi OPEN): per id coin atau per simbol.
        .route("/holding", get(get_holding))
        .route("/holding/:symbol", get(get_holding_by_symbol))
        .route("/:id", get(get_trade_position_by_id))
        .route_layer(middleware::from_fn(authenticate))
}
