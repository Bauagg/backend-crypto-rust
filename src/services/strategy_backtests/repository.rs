use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde_json::Value;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::model::StrategyBacktest;
use crate::utils::app_error::AppError;

pub struct NewBacktest<'a> {
    pub strategy_version_id: Uuid,
    pub user_id: Uuid,
    pub input_values: &'a Value,
    pub period_start: NaiveDate,
    pub period_end: NaiveDate,
    pub initial_capital: Decimal,
    pub fee_rate: Decimal,
    pub status: &'a str,
    pub stats: &'a Value,
    pub robustness: &'a Value,
    pub equity_curve: &'a Value,
    pub analysis: &'a Value,
    pub is_official: bool,
    pub validation: &'a Value,
    pub insights: &'a Value,
    pub error: Option<&'a str>,
    pub duration_ms: i32,
}

pub async fn create_backtest(
    tx: &mut Transaction<'_, Postgres>,
    new: &NewBacktest<'_>,
    actor: &str,
) -> Result<StrategyBacktest, AppError> {
    let backtest = sqlx::query_as::<_, StrategyBacktest>(
        r#"
        INSERT INTO strategy_backtests
            (strategy_version_id, user_id, input_values, period_start, period_end, initial_capital,
             fee_rate, status, stats, robustness, equity_curve, analysis, is_official, validation,
             insights, error, duration_ms, created_by)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)
        RETURNING *
        "#,
    )
    .bind(new.strategy_version_id)
    .bind(new.user_id)
    .bind(new.input_values)
    .bind(new.period_start)
    .bind(new.period_end)
    .bind(new.initial_capital)
    .bind(new.fee_rate)
    .bind(new.status)
    .bind(new.stats)
    .bind(new.robustness)
    .bind(new.equity_curve)
    .bind(new.analysis)
    .bind(new.is_official)
    .bind(new.validation)
    .bind(new.insights)
    .bind(new.error)
    .bind(new.duration_ms)
    .bind(actor)
    .fetch_one(&mut **tx)
    .await?;
    Ok(backtest)
}

/// Backtest + nomor versi & id strategi-nya.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct BacktestWithVersion {
    #[sqlx(flatten)]
    pub backtest: StrategyBacktest,
    pub strategy_id: Uuid,
    pub version_no: i32,
}

/// 1 backtest + versinya.
pub async fn find_backtest_with_version(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
) -> Result<Option<BacktestWithVersion>, AppError> {
    let row = sqlx::query_as::<_, BacktestWithVersion>(
        r#"
        SELECT b.*, v.strategy_id, v.version_no FROM strategy_backtests b
        JOIN strategy_versions v ON v.id = b.strategy_version_id
        WHERE b.id = $1
        "#,
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(row)
}

/// Backtest semua versi 1 strategi yang dijalankan salah satu `user_ids`, terbaru dulu (maks 50).
pub async fn find_backtests_of_strategy(
    tx: &mut Transaction<'_, Postgres>,
    strategy_id: Uuid,
    user_ids: &[Uuid],
) -> Result<Vec<BacktestWithVersion>, AppError> {
    let rows = sqlx::query_as::<_, BacktestWithVersion>(
        r#"
        SELECT b.*, v.strategy_id, v.version_no FROM strategy_backtests b
        JOIN strategy_versions v ON v.id = b.strategy_version_id
        WHERE v.strategy_id = $1 AND b.user_id = ANY($2)
        ORDER BY b.created_at DESC
        LIMIT 50
        "#,
    )
    .bind(strategy_id)
    .bind(user_ids)
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows)
}

/// Backtest resmi terbaru 1 versi: dijalankan `owner_id` (pemilik strategi) dengan modal &
/// setelan standar (`is_official`), status DONE. Dipakai statistik Store.
pub async fn find_latest_official_backtest(
    tx: &mut Transaction<'_, Postgres>,
    version_id: Uuid,
    owner_id: Uuid,
) -> Result<Option<StrategyBacktest>, AppError> {
    let backtest = sqlx::query_as::<_, StrategyBacktest>(
        r#"
        SELECT * FROM strategy_backtests
        WHERE strategy_version_id = $1 AND user_id = $2 AND is_official AND status = 'DONE'
        ORDER BY created_at DESC LIMIT 1
        "#,
    )
    .bind(version_id)
    .bind(owner_id)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(backtest)
}
