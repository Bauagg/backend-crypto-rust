use axum::{middleware, routing::get, Router};
use sqlx::PgPool;

use super::controller::{
    create_subscription, delete_subscription, get_subscription_performance, get_subscription_results,
    get_subscriptions, update_subscription,
};
use crate::middlewares::authenticate::authenticate;

pub fn router() -> Router<PgPool> {
    Router::new()
        .route("/", get(get_subscriptions).post(create_subscription))
        .route("/:id", axum::routing::put(update_subscription).delete(delete_subscription))
        .route("/:id/results", get(get_subscription_results))
        .route("/:id/performance", get(get_subscription_performance))
        .route_layer(middleware::from_fn(authenticate))
}
