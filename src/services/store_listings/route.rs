use axum::{middleware, routing::get, Router};
use sqlx::PgPool;

use super::controller::{browse_listings, create_listing, delete_listing, get_listing, my_listings, update_listing};
use crate::middlewares::authenticate::authenticate;

pub fn router() -> Router<PgPool> {
    Router::new()
        .route("/", get(browse_listings).post(create_listing))
        .route("/mine", get(my_listings))
        .route("/:id", get(get_listing).put(update_listing).delete(delete_listing))
        .route_layer(middleware::from_fn(authenticate))
}
