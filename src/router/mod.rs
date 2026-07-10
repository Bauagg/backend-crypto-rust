use axum::Router;
use sqlx::PgPool;

use crate::services::{candle_market, documents, fear_greed, flex_params, market, users};

/// Kumpulan seluruh route service didaftarkan di sini, lalu di-nest di `main.rs` di bawah prefix `/api`.
pub fn router() -> Router<PgPool> {
    Router::new()
        .nest("/users", users::route::router())
        .nest("/files", documents::route::router())
        .nest("/flex-params", flex_params::route::router())
        .nest("/market", market::route::router())
        .nest("/candle-market", candle_market::route::router())
        .nest("/fear-greed", fear_greed::route::router())
}
