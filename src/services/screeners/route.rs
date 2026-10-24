use axum::{
    middleware,
    routing::{get, post},
    Router,
};
use sqlx::PgPool;

use super::controller::{ai_chat, delete_screener, get_screener, get_screening_results, save_screener};
use crate::middlewares::authenticate::authenticate;

/// 1 user = 1 strategi, jadi tidak ada `:id` di path — semuanya milik user yang login.
pub fn router() -> Router<PgPool> {
    Router::new()
        .route("/", get(get_screener).put(save_screener).delete(delete_screener))
        .route("/results", get(get_screening_results))
        .route("/ai-chat", post(ai_chat))
        .route_layer(middleware::from_fn(authenticate))
}
