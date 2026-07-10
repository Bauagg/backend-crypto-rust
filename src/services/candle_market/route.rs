use axum::{routing::get, Router};
use sqlx::PgPool;

use super::controller::get_stored_candles;

pub fn router() -> Router<PgPool> {
    Router::new().route("/", get(get_stored_candles))
}
