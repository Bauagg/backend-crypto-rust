use serde_json::Value;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::model::StrategySubscription;
use crate::utils::app_error::AppError;

pub async fn find_subscriptions_of_user(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
) -> Result<Vec<StrategySubscription>, AppError> {
    let rows = sqlx::query_as::<_, StrategySubscription>(
        "SELECT * FROM strategy_subscriptions WHERE user_id = $1 AND deleted_at IS NULL ORDER BY created_at DESC",
    )
    .bind(user_id)
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows)
}

/// Pemakaian milik `user_id` — milik user lain dianggap tidak ada.
pub async fn find_subscription_of_user(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    user_id: Uuid,
) -> Result<Option<StrategySubscription>, AppError> {
    let row = sqlx::query_as::<_, StrategySubscription>(
        "SELECT * FROM strategy_subscriptions WHERE id = $1 AND user_id = $2 AND deleted_at IS NULL",
    )
    .bind(id)
    .bind(user_id)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(row)
}

pub async fn exists_subscription(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    strategy_id: Uuid,
    usage: &str,
    account_mode: Option<&str>,
) -> Result<bool, AppError> {
    let exists = sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1 FROM strategy_subscriptions
            WHERE user_id = $1 AND strategy_id = $2 AND usage = $3
              AND COALESCE(account_mode, '') = COALESCE($4, '') AND deleted_at IS NULL
        )
        "#,
    )
    .bind(user_id)
    .bind(strategy_id)
    .bind(usage)
    .bind(account_mode)
    .fetch_one(&mut **tx)
    .await?;
    Ok(exists)
}

pub struct NewSubscription<'a> {
    pub user_id: Uuid,
    pub strategy_id: Uuid,
    pub strategy_version_id: Uuid,
    pub usage: &'a str,
    pub account_mode: Option<&'a str>,
    pub capital_percent: Option<rust_decimal::Decimal>,
    pub input_values: &'a Value,
}

pub async fn create_subscription(
    tx: &mut Transaction<'_, Postgres>,
    new: &NewSubscription<'_>,
    actor: &str,
) -> Result<StrategySubscription, AppError> {
    let row = sqlx::query_as::<_, StrategySubscription>(
        r#"
        INSERT INTO strategy_subscriptions
            (user_id, strategy_id, strategy_version_id, usage, account_mode, capital_percent,
             input_values, created_by, updated_by)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $8)
        RETURNING *
        "#,
    )
    .bind(new.user_id)
    .bind(new.strategy_id)
    .bind(new.strategy_version_id)
    .bind(new.usage)
    .bind(new.account_mode)
    .bind(new.capital_percent)
    .bind(new.input_values)
    .bind(actor)
    .fetch_one(&mut **tx)
    .await?;
    Ok(row)
}

pub struct SubscriptionUpdate<'a> {
    pub strategy_version_id: Uuid,
    pub input_values: &'a Value,
    pub capital_percent: Option<rust_decimal::Decimal>,
    pub is_active: bool,
}

pub async fn update_subscription(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    update: &SubscriptionUpdate<'_>,
    actor: &str,
) -> Result<StrategySubscription, AppError> {
    let row = sqlx::query_as::<_, StrategySubscription>(
        r#"
        UPDATE strategy_subscriptions
        SET strategy_version_id = $2, input_values = $3, capital_percent = $4, is_active = $5,
            updated_by = $6, updated_at = now()
        WHERE id = $1 AND deleted_at IS NULL
        RETURNING *
        "#,
    )
    .bind(id)
    .bind(update.strategy_version_id)
    .bind(update.input_values)
    .bind(update.capital_percent)
    .bind(update.is_active)
    .bind(actor)
    .fetch_one(&mut **tx)
    .await?;
    Ok(row)
}

pub async fn soft_delete_subscription(tx: &mut Transaction<'_, Postgres>, id: Uuid, actor: &str) -> Result<(), AppError> {
    sqlx::query(
        r#"
        UPDATE strategy_subscriptions SET deleted_at = now(), deleted_by = $2, updated_at = now(), updated_by = $2
        WHERE id = $1 AND deleted_at IS NULL
        "#,
    )
    .bind(id)
    .bind(actor)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Bot aktif yang belum dijalankan pada `today` — dikerjakan worker `strategy-bot`.
pub async fn find_bots_to_run(
    tx: &mut Transaction<'_, Postgres>,
    today: chrono::NaiveDate,
) -> Result<Vec<StrategySubscription>, AppError> {
    let rows = sqlx::query_as::<_, StrategySubscription>(
        r#"
        SELECT * FROM strategy_subscriptions
        WHERE usage = 'BOT' AND is_active AND deleted_at IS NULL
          AND (last_run_date IS NULL OR last_run_date < $1)
        ORDER BY created_at
        "#,
    )
    .bind(today)
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows)
}

/// Tandai bot sudah dijalankan hari ini (order maksimal 1x per hari).
pub async fn mark_bot_run(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    today: chrono::NaiveDate,
) -> Result<(), AppError> {
    sqlx::query("UPDATE strategy_subscriptions SET last_run_date = $2, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(today)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Hentikan semua pemakaian 1 strategi (strategi dihapus pemiliknya).
pub async fn soft_delete_subscriptions_of_strategy(
    tx: &mut Transaction<'_, Postgres>,
    strategy_id: Uuid,
    actor: &str,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        UPDATE strategy_subscriptions SET deleted_at = now(), deleted_by = $2, updated_at = now(), updated_by = $2
        WHERE strategy_id = $1 AND deleted_at IS NULL
        "#,
    )
    .bind(strategy_id)
    .bind(actor)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
