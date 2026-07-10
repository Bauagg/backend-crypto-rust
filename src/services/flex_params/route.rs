use axum::{
    middleware,
    routing::{get, post, put},
    Router,
};
use sqlx::PgPool;

use super::controller::{
    create_flex_param, delete_flex_param, get_all_flex_params, get_flex_param_by_id,
    get_flex_params_by_header_id, get_flex_params_by_type, update_flex_param,
};
use crate::middlewares::authenticate::authenticate;

pub fn router() -> Router<PgPool> {
    let protected = Router::new()
        .route("/", post(create_flex_param))
        .route("/:id", put(update_flex_param).delete(delete_flex_param))
        .route_layer(middleware::from_fn(authenticate));

    Router::new()
        .route("/", get(get_all_flex_params))
        .route("/type/:type_param", get(get_flex_params_by_type))
        .route("/header/:header_id", get(get_flex_params_by_header_id))
        .route("/:id", get(get_flex_param_by_id))
        .merge(protected)
}
