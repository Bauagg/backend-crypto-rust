//! Mesin robot trading harian — dijadwalkan worker `robot` (`src/worker/robot.rs`), aturannya
//! mengikuti `backtes-crypto/docs/API_RUST.md` bagian 4:
//! - `demo` — akun DEMO: order disimulasikan dengan hitungan Binance, saldo di `users.demo_balance`.
//! - `live` — akun LIVE: order MARKET asli ke Binance (atau testnet, `ACCOUNT_API_BASE_URL`).
//! - `strategy` — robot dari strategi script buatan user (`strategy_subscriptions` usage BOT),
//!   dijadwalkan worker `strategy-bot`; tiap bot hanya mengelola posisinya sendiri.
//! - `common` — hitungan eksekusi yang sama untuk robot & BUY/SELL manual (`services/transactions`).
//!
//! Hasil robot dicatat di `trade_positions` dengan `source = BOT`.

pub mod common;
mod demo;
mod live;
pub mod strategy;

pub use demo::{run_demo_robot, DemoRobotRun};
pub use live::{run_live_robot, LiveRobotRun};
pub use strategy::{run_strategy_bots, StrategyBotRun};
