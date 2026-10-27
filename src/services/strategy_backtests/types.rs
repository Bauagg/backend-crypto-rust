use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use super::model::StrategyBacktest;

/// Body `POST /api/strategy-backtests`.
#[derive(Debug, Deserialize)]
pub struct RunBacktestInput {
    pub strategy_id: Uuid,
    /// Versi yang diuji (khusus pemilik strategi); kosong = versi aktif.
    #[serde(default)]
    pub strategy_version_id: Option<Uuid>,
    /// Nilai setelan script (kosong = default).
    #[serde(default)]
    pub input_values: Value,
    /// Modal awal simulasi dalam USDT, 10–1.000.000 (default 1.000). Backtest resmi untuk Store
    /// hanya yang memakai modal 1.000 & setelan default.
    #[serde(default)]
    pub initial_capital: Option<Decimal>,
}

/// Query `GET /api/strategy-backtests`.
#[derive(Debug, Deserialize)]
pub struct BacktestListQuery {
    pub strategy_id: Uuid,
}

/// Backtest + info versi. `equity_curve` hanya di detail (bisa ratusan titik).
#[derive(Debug, Serialize)]
pub struct BacktestView {
    #[serde(flatten)]
    pub backtest: StrategyBacktest,
    pub strategy_id: Uuid,
    pub version_no: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub equity_curve: Option<Value>,
}

/// Ringkasan backtest resmi (dijalankan pemilik, setelan default) — tampil di detail strategi & Store.
#[derive(Debug, Serialize)]
pub struct BacktestSummary {
    pub id: Uuid,
    pub period_start: NaiveDate,
    pub period_end: NaiveDate,
    /// Return, CAGR, max drawdown, win rate, profit factor, Sharpe, vs BTC, return per bulan.
    pub stats: Value,
    /// Label KUAT/SEDANG/LEMAH + paruh awal vs akhir, bulan profit, drawdown terlama.
    pub robustness: Value,
    /// `{"strengths": [...], "weaknesses": [...]}`
    pub analysis: Value,
    /// Uji overfitting + skor risiko RENDAH/SEDANG/TINGGI.
    pub validation: Value,
    /// Faktor yang paling memengaruhi untung/rugi + saran.
    pub insights: Value,
    pub created_at: DateTime<Utc>,
}

impl From<StrategyBacktest> for BacktestSummary {
    fn from(b: StrategyBacktest) -> Self {
        Self {
            id: b.id,
            period_start: b.period_start,
            period_end: b.period_end,
            stats: b.stats,
            robustness: b.robustness,
            analysis: b.analysis,
            validation: b.validation,
            insights: b.insights,
            created_at: b.created_at,
        }
    }
}

/// Query `GET /api/strategy-backtests/:id/trades`.
#[derive(Debug, Default, Deserialize)]
pub struct TradesQuery {
    pub page: Option<i64>,
    pub limit: Option<i64>,
}
