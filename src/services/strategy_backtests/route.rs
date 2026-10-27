use axum::{middleware, routing::get, Router};
use sqlx::PgPool;

use super::controller::{get_backtest, get_backtest_trades, get_backtests, run_backtest};
use crate::middlewares::authenticate::authenticate;

pub fn router() -> Router<PgPool> {
    Router::new()
        .route("/", get(get_backtests).post(run_backtest))
        .route("/:id", get(get_backtest))
        .route("/:id/trades", get(get_backtest_trades))
        .route_layer(middleware::from_fn(authenticate))
}
