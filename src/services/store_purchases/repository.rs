use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::model::StorePurchase;
use crate::utils::app_error::AppError;

/// Kondisi hak pakai yang masih berlaku.
const ACTIVE: &str = "status = 'ACTIVE' AND (expires_at IS NULL OR expires_at > now())";

/// Hak pakai strategi yang masih berlaku milik user (terbaru dulu) — `None` = tidak punya.
pub async fn find_active_purchase(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    strategy_id: Uuid,
) -> Result<Option<StorePurchase>, AppError> {
    let purchase = sqlx::query_as::<_, StorePurchase>(&format!(
        "SELECT * FROM store_purchases WHERE user_id = $1 AND strategy_id = $2 AND {ACTIVE} \
         ORDER BY expires_at DESC NULLS FIRST LIMIT 1"
    ))
    .bind(user_id)
    .bind(strategy_id)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(purchase)
}

/// Hak pakai yang masih berlaku milik user untuk beberapa strategi sekaligus.
pub async fn find_active_purchases_for(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    strategy_ids: &[Uuid],
) -> Result<Vec<StorePurchase>, AppError> {
    let purchases = sqlx::query_as::<_, StorePurchase>(&format!(
        "SELECT * FROM store_purchases WHERE user_id = $1 AND strategy_id = ANY($2) AND {ACTIVE}"
    ))
    .bind(user_id)
    .bind(strategy_ids)
    .fetch_all(&mut **tx)
    .await?;
    Ok(purchases)
}

pub async fn has_used_trial(tx: &mut Transaction<'_, Postgres>, user_id: Uuid, listing_id: Uuid) -> Result<bool, AppError> {
    let used = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM store_purchases WHERE user_id = $1 AND listing_id = $2 AND kind = 'TRIAL')",
    )
    .bind(user_id)
    .bind(listing_id)
    .fetch_one(&mut **tx)
    .await?;
    Ok(used)
}

/// Semua pembelian/trial user, terbaru dulu.
pub async fn find_purchases_of_user(tx: &mut Transaction<'_, Postgres>, user_id: Uuid) -> Result<Vec<StorePurchase>, AppError> {
    let purchases = sqlx::query_as::<_, StorePurchase>(
        "SELECT * FROM store_purchases WHERE user_id = $1 ORDER BY created_at DESC",
    )
    .bind(user_id)
    .fetch_all(&mut **tx)
    .await?;
    Ok(purchases)
}

/// Jumlah pemakai yang punya hak pakai berlaku per listing (statistik Store).
pub async fn count_active_buyers(tx: &mut Transaction<'_, Postgres>, listing_ids: &[Uuid]) -> Result<Vec<(Uuid, i64)>, AppError> {
    let rows = sqlx::query_as(&format!(
        "SELECT listing_id, COUNT(DISTINCT user_id) FROM store_purchases WHERE listing_id = ANY($1) AND {ACTIVE} \
         GROUP BY listing_id"
    ))
    .bind(listing_ids)
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows)
}

pub struct NewPurchase<'a> {
    pub user_id: Uuid,
    pub listing_id: Uuid,
    pub strategy_id: Uuid,
    pub kind: &'a str,
    pub pricing_model: &'a str,
    pub list_price_idr: Decimal,
    pub discount_idr: Decimal,
    pub paid_idr: Decimal,
    pub promo_id: Option<Uuid>,
    pub payment_status: &'a str,
    pub expires_at: Option<DateTime<Utc>>,
}

pub async fn create_purchase(
    tx: &mut Transaction<'_, Postgres>,
    new: &NewPurchase<'_>,
    actor: &str,
) -> Result<StorePurchase, AppError> {
    let purchase = sqlx::query_as::<_, StorePurchase>(
        r#"
        INSERT INTO store_purchases
            (user_id, listing_id, strategy_id, kind, pricing_model, list_price_idr, discount_idr,
             paid_idr, promo_id, payment_status, expires_at, created_by, updated_by)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $12)
        RETURNING *
        "#,
    )
    .bind(new.user_id)
    .bind(new.listing_id)
    .bind(new.strategy_id)
    .bind(new.kind)
    .bind(new.pricing_model)
    .bind(new.list_price_idr)
    .bind(new.discount_idr)
    .bind(new.paid_idr)
    .bind(new.promo_id)
    .bind(new.payment_status)
    .bind(new.expires_at)
    .bind(actor)
    .fetch_one(&mut **tx)
    .await?;
    Ok(purchase)
}
