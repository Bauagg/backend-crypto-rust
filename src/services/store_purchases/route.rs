use axum::{middleware, routing::get, routing::post, Router};
use sqlx::PgPool;

use super::controller::{checkout, my_purchases, quote};
use crate::middlewares::authenticate::authenticate;

pub fn router() -> Router<PgPool> {
    Router::new()
        .route("/", get(my_purchases).post(checkout))
        .route("/quote", post(quote))
        .route_layer(middleware::from_fn(authenticate))
}
