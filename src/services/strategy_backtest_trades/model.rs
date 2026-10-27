use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::Serialize;
use uuid::Uuid;

/// 1 baris `strategy_backtest_trades` — 1 trade simulasi (beli -> jual 1 coin).
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct StrategyBacktestTrade {
    pub id: Uuid,
    pub backtest_id: Uuid,
    pub symbol: String,
    pub entry_date: NaiveDate,
    /// `None` = masih dipegang saat backtest berakhir.
    pub exit_date: Option<NaiveDate>,
    pub entry_price: Decimal,
    pub exit_price: Decimal,
    pub quantity: Decimal,
    pub entry_value: Decimal,
    pub exit_value: Decimal,
    pub fee_amount: Decimal,
    pub pnl_amount: Decimal,
    pub pnl_percent: Decimal,
    pub holding_days: i32,
    pub entry_score: Decimal,
    pub exit_score: Option<Decimal>,
    pub entry_reason: Option<String>,
    pub exit_reason: Option<String>,
}
