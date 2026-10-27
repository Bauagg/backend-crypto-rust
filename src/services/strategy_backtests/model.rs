use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

/// 1 baris `strategy_backtests` — hasil uji 1 versi strategi ke histori candle harian.
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct StrategyBacktest {
    pub id: Uuid,
    pub strategy_version_id: Uuid,
    pub user_id: Uuid,
    pub input_values: Value,
    pub period_start: NaiveDate,
    pub period_end: NaiveDate,
    pub initial_capital: Decimal,
    pub fee_rate: Decimal,
    /// `DONE` | `FAILED`
    pub status: String,
    pub stats: Value,
    pub robustness: Value,
    #[serde(skip_serializing)]
    pub equity_curve: Value,
    /// `{"strengths": [...], "weaknesses": [...]}`
    pub analysis: Value,
    /// Dijalankan pemilik strategi, modal 1.000 USDT, setelan default = statistik resmi Store.
    pub is_official: bool,
    /// Uji overfitting: out-of-sample, Monte Carlo, banding acak, sensitivitas + skor risiko.
    pub validation: Value,
    /// Faktor yang paling memengaruhi untung/rugi trade + saran.
    pub insights: Value,
    pub error: Option<String>,
    pub duration_ms: i32,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
}
