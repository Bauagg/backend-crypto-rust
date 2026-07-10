use axum::{
    middleware,
    routing::{get, post},
    Router,
};
use sqlx::PgPool;

use super::controller::{delete_file, get_file, update_file, upload_file};
use crate::middlewares::authenticate::authenticate;

pub fn router() -> Router<PgPool> {
    Router::new()
        .route("/upload", post(upload_file))
        .route("/:id", get(get_file).put(update_file).delete(delete_file))
        .route_layer(middleware::from_fn(authenticate))
}
