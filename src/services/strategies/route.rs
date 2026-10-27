use axum::{
    middleware,
    routing::{get, post},
    Router,
};
use sqlx::PgPool;

use super::controller::{
    ai_chat, create_strategy, create_version, delete_strategy, get_my_strategies, get_script_reference,
    get_strategy, get_strategy_results, get_versions, update_strategy, validate_script,
};
use crate::middlewares::authenticate::authenticate;

pub fn router() -> Router<PgPool> {
    Router::new()
        .route("/", get(get_my_strategies).post(create_strategy))
        .route("/reference", get(get_script_reference))
        .route("/validate", post(validate_script))
        .route("/ai-chat", post(ai_chat))
        .route("/:id", get(get_strategy).put(update_strategy).delete(delete_strategy))
        .route("/:id/versions", get(get_versions).post(create_version))
        .route("/:id/results", get(get_strategy_results))
        .route_layer(middleware::from_fn(authenticate))
}
