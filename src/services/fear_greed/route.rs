use axum::{routing::get, Router};
use sqlx::PgPool;

use super::controller::get_fng;

pub fn router() -> Router<PgPool> {
    Router::new().route("/", get(get_fng))
}
