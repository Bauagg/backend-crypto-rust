use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

/// 1 baris `strategy_subscriptions` — user memakai 1 strategi (terkunci ke 1 versi) sebagai
/// screener atau bot.
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct StrategySubscription {
    pub id: Uuid,
    pub user_id: Uuid,
    pub strategy_id: Uuid,
    pub strategy_version_id: Uuid,
    /// `SCREENER` | `BOT`
    pub usage: String,
    /// `DEMO` | `LIVE` untuk BOT; `None` untuk SCREENER.
    pub account_mode: Option<String>,
    pub input_values: Value,
    /// Persen saldo akun yang boleh dipakai bot ini (wajib untuk BOT).
    pub capital_percent: Option<Decimal>,
    pub is_active: bool,
    /// Tanggal (UTC) terakhir worker menjalankan bot ini.
    pub last_run_date: Option<NaiveDate>,
    pub created_by: String,
    pub updated_by: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
