//! Semua WebSocket real-time di satu tempat — terpisah dari `services` (logika bisnis REST) dan
//! `worker` (job berjadwal). Pola seperti `worker/mod.rs`: hub dibuat & dijalankan di sini, lalu
//! `main.rs` cukup memanggil `websocket::start` dan memasang `websocket::router`.
//!
//! ```text
//! Binance ──wss──► KlineHub  (1 koneksi per simbol+interval yang sedang ditonton) ──► WS /api/market/ws
//! Binance ──wss──► TickerHub (1 koneksi untuk semua coin aktif, jalan terus)      ──► WS /api/market/ws/tickers
//! ```
//!
//! | File            | Isi                                                                  |
//! |-----------------|----------------------------------------------------------------------|
//! | `kline_hub.rs`  | candle live chart, fan-out ke semua client + tulis ke cache Redis    |
//! | `ticker_hub.rs` | harga & % 24 jam semua coin, disimpan di memori + disiarkan tiap 1 dtk |
//! | `handler.rs`    | endpoint WebSocket untuk FE + relay pesan hub ke client              |
//! | `types.rs`      | query & bentuk pesan WebSocket                                       |

mod handler;
mod kline_hub;
mod ticker_hub;
mod types;

use std::sync::Arc;

use axum::{routing::get, Extension, Router};
use sqlx::PgPool;

use crate::database::RedisPool;

pub use kline_hub::KlineHub;
pub use ticker_hub::TickerHub;
pub use types::MarketTicker;

/// Hub yang dipakai bersama seluruh aplikasi — dipasang sebagai `Extension` di router supaya
/// endpoint (WebSocket maupun REST seperti `/market/symbols` & dashboard) bisa memakainya.
#[derive(Clone)]
pub struct Hubs {
    pub kline: Arc<KlineHub>,
    pub ticker: Arc<TickerHub>,
}

impl Hubs {
    /// Pasang kedua hub sebagai `Extension` di router aplikasi.
    pub fn layer<S: Clone + Send + Sync + 'static>(&self, router: Router<S>) -> Router<S> {
        router
            .layer(Extension(self.kline.clone()))
            .layer(Extension(self.ticker.clone()))
    }
}

/// Buat semua hub & jalankan stream yang harus hidup terus (TickerHub). KlineHub tidak perlu
/// dijalankan — koneksinya dibuka otomatis saat ada client yang membuka chart.
pub fn start(pool: PgPool, redis: RedisPool) -> Hubs {
    let kline = KlineHub::new(redis);
    let ticker = TickerHub::new();
    tokio::spawn(ticker.clone().run(pool));
    Hubs { kline, ticker }
}

/// Endpoint WebSocket — di-nest di bawah `/api/market` (URL untuk FE tetap sama):
/// `WS /api/market/ws?symbol=&interval=` & `WS /api/market/ws/tickers?symbols=`.
pub fn router() -> Router<PgPool> {
    Router::new()
        .route("/ws", get(handler::ws_klines))
        .route("/ws/tickers", get(handler::ws_tickers))
}
