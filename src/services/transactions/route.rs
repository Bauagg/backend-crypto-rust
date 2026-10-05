use axum::{middleware, routing::get, Router};
use sqlx::PgPool;

use super::controller::{
    create_trade_position, get_dashboard, get_trade_position_by_id, get_trade_positions,
};
use crate::middlewares::authenticate::authenticate;

pub fn router() -> Router<PgPool> {
    Router::new()
        .route("/", get(get_trade_positions).post(create_trade_position))
        .route("/dashboard", get(get_dashboard))
        .route("/:id", get(get_trade_position_by_id))
        .route_layer(middleware::from_fn(authenticate))
}
