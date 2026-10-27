use axum::Router;
use sqlx::PgPool;

use crate::services::{
    candle_ohlcv, documents, fear_greed, flex_params, market, screeners, store_listings, store_promos,
    store_purchases, strategies, strategy_backtests, strategy_subscriptions, transactions, users,
};

/// Kumpulan seluruh route service didaftarkan di sini, lalu di-nest di `main.rs` di bawah prefix `/api`.
pub fn router() -> Router<PgPool> {
    Router::new()
        .nest("/users", users::route::router())
        .nest("/files", documents::route::router())
        .nest("/flex-params", flex_params::route::router())
        // REST market + WebSocket real-time (modul `websocket`) di prefix yang sama.
        .nest("/market", market::route::router().merge(crate::websocket::router()))
        .nest("/candle-ohlcv", candle_ohlcv::route::router())
        .nest("/fear-greed", fear_greed::route::router())
        .nest("/transactions", transactions::route::router())
        .nest("/screeners", screeners::route::router())
        .nest("/strategies", strategies::route::router())
        .nest("/strategy-subscriptions", strategy_subscriptions::route::router())
        .nest("/strategy-backtests", strategy_backtests::route::router())
        .nest("/store/listings", store_listings::route::router())
        .nest("/store/promos", store_promos::route::router())
        .nest("/store/purchases", store_purchases::route::router())
}
