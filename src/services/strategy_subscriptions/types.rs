use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::model::StrategySubscription;
use crate::services::strategy_versions::types::BotConfig;
use crate::services::transactions::types::LivePerformance;

/// Body `POST /api/strategy-subscriptions` — pakai strategi (milik sendiri atau PUBLIC).
#[derive(Debug, Deserialize)]
pub struct CreateSubscriptionInput {
    pub strategy_id: Uuid,
    /// `SCREENER` (default, hanya strategi semua coin) | `BOT` (robot order otomatis).
    #[serde(default)]
    pub usage: Option<String>,
    /// BOT: akun tempat robot berjalan, `DEMO` | `LIVE`. Mau keduanya = buat 2 pemakaian.
    #[serde(default)]
    pub account_mode: Option<String>,
    /// BOT: persen saldo akun yang boleh dipakai robot ini (1–100), mis. 10.
    #[serde(default)]
    pub capital_percent: Option<Decimal>,
    /// Nilai setelan strategi, mis. `{"rsi_period": 10}` (kosong = default).
    #[serde(default)]
    pub input_values: Value,
}

/// Body `PUT /api/strategy-subscriptions/:id`.
#[derive(Debug, Deserialize)]
pub struct UpdateSubscriptionInput {
    #[serde(default)]
    pub input_values: Option<Value>,
    #[serde(default)]
    pub is_active: Option<bool>,
    /// BOT: ubah persen saldo yang boleh dipakai (1–100).
    #[serde(default)]
    pub capital_percent: Option<Decimal>,
    /// `true` = pindah ke versi terbaru strategi (setelan disesuaikan ulang ke versi itu).
    #[serde(default)]
    pub upgrade_to_latest: bool,
}

/// Pemakaian strategi + info singkat strategi & versinya (tanpa isi script).
#[derive(Debug, Serialize)]
pub struct SubscriptionDetail {
    #[serde(flatten)]
    pub subscription: StrategySubscription,
    pub strategy_name: String,
    pub version_no: i32,
    /// Versi terbaru strategi kalau lebih baru dari versi yang dipakai (bisa `upgrade_to_latest`).
    pub latest_version_no: Option<i32>,
    /// Deklarasi setelan versi yang dipakai — untuk form ubah `input_values` di FE.
    pub inputs: Value,
    /// Aturan robot & cakupan coin versi yang dipakai.
    pub bot: Option<BotConfig>,
}

/// Performa asli 1 bot (dari transaksinya).
#[derive(Debug, Serialize)]
pub struct SubscriptionPerformance {
    pub subscription_id: Uuid,
    pub performance: Vec<LivePerformance>,
}
