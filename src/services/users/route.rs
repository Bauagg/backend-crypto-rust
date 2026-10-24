use axum::{middleware, routing::post, Router};
use sqlx::PgPool;

use super::controller::{
    forgot_password, get_profile, login, refresh_token, register, reset_password, update_profile,
    verify_otp,
};
use crate::middlewares::authenticate::authenticate;

pub fn router() -> Router<PgPool> {
    let protected = Router::new()
        .route("/profile", axum::routing::get(get_profile).put(update_profile))
        .route_layer(middleware::from_fn(authenticate));

    Router::new()
        .route("/register", post(register))
        .route("/login", post(login))
        .route("/refresh-token", post(refresh_token))
        // Lupa password: email -> OTP 6 digit -> password baru.
        .route("/forgot-password", post(forgot_password))
        .route("/verify-otp", post(verify_otp))
        .route("/reset-password", post(reset_password))
        .merge(protected)
}
