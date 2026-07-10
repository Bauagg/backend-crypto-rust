use axum::{middleware, routing::post, Router};
use sqlx::PgPool;

use super::controller::{get_profile, login, refresh_token, register, update_profile};
use crate::middlewares::authenticate::authenticate;

pub fn router() -> Router<PgPool> {
    let protected = Router::new()
        .route("/profile", axum::routing::get(get_profile).put(update_profile))
        .route_layer(middleware::from_fn(authenticate));

    Router::new()
        .route("/register", post(register))
        .route("/login", post(login))
        .route("/refresh-token", post(refresh_token))
        .merge(protected)
}
