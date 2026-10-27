use axum::{middleware, routing::get, routing::put, Router};
use sqlx::PgPool;

use super::controller::{create_promo, delete_promo, list_promos, update_promo};
use crate::middlewares::authenticate::authenticate;

pub fn router() -> Router<PgPool> {
    Router::new()
        .route("/", get(list_promos).post(create_promo))
        .route("/:id", put(update_promo).delete(delete_promo))
        .route_layer(middleware::from_fn(authenticate))
}
