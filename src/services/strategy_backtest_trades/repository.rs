use rust_decimal::prelude::FromPrimitive;
use rust_decimal::Decimal;
use sqlx::{Postgres, QueryBuilder, Transaction};
use uuid::Uuid;

use super::model::StrategyBacktestTrade;
use crate::strategy_engine::BacktestTrade;
use crate::utils::app_error::AppError;

/// Batas baris per 1 query insert (Postgres maks 65535 parameter, 1 trade = 17 parameter).
const INSERT_CHUNK: usize = 1000;

fn dec(value: f64, scale: u32) -> Decimal {
    Decimal::from_f64(value).unwrap_or_default().round_dp(scale)
}

/// Simpan semua trade 1 backtest (di transaksi pemanggil).
pub async fn create_backtest_trades(
    tx: &mut Transaction<'_, Postgres>,
    backtest_id: Uuid,
    trades: &[BacktestTrade],
) -> Result<(), AppError> {
    for chunk in trades.chunks(INSERT_CHUNK) {
        let mut query = QueryBuilder::<Postgres>::new(
            "INSERT INTO strategy_backtest_trades (backtest_id, symbol, entry_date, exit_date, entry_price, \
             exit_price, quantity, entry_value, exit_value, fee_amount, pnl_amount, pnl_percent, holding_days, \
             entry_score, exit_score, entry_reason, exit_reason) ",
        );
        query.push_values(chunk, |mut row, t| {
            row.push_bind(backtest_id)
                .push_bind(&t.symbol)
                .push_bind(t.entry_date)
                .push_bind(t.exit_date)
                .push_bind(dec(t.entry_price, 8))
                .push_bind(dec(t.exit_price, 8))
                .push_bind(dec(t.quantity, 8))
                .push_bind(dec(t.entry_value, 8))
                .push_bind(dec(t.exit_value, 8))
                .push_bind(dec(t.fee_amount, 8))
                .push_bind(dec(t.pnl_amount, 8))
                .push_bind(dec(t.pnl_percent, 4))
                .push_bind(t.holding_days as i32)
                .push_bind(dec(t.entry_score, 2))
                .push_bind(t.exit_score.map(|s| dec(s, 2)))
                .push_bind(&t.entry_reason)
                .push_bind(&t.exit_reason);
        });
        query.build().execute(&mut **tx).await?;
    }
    Ok(())
}

/// Trade 1 backtest, urut tanggal masuk, dengan pagination. Balik `(data, total)`.
pub async fn find_backtest_trades(
    tx: &mut Transaction<'_, Postgres>,
    backtest_id: Uuid,
    limit: i64,
    offset: i64,
) -> Result<(Vec<StrategyBacktestTrade>, i64), AppError> {
    let trades = sqlx::query_as::<_, StrategyBacktestTrade>(
        r#"
        SELECT * FROM strategy_backtest_trades WHERE backtest_id = $1
        ORDER BY entry_date, symbol LIMIT $2 OFFSET $3
        "#,
    )
    .bind(backtest_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(&mut **tx)
    .await?;
    let total = sqlx::query_scalar("SELECT COUNT(*) FROM strategy_backtest_trades WHERE backtest_id = $1")
        .bind(backtest_id)
        .fetch_one(&mut **tx)
        .await?;
    Ok((trades, total))
}
