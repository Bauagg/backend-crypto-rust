use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::model::StorePromo;
use crate::utils::app_error::AppError;

pub async fn find_promos_of_listing(tx: &mut Transaction<'_, Postgres>, listing_id: Uuid) -> Result<Vec<StorePromo>, AppError> {
    let promos = sqlx::query_as::<_, StorePromo>(
        "SELECT * FROM store_promos WHERE listing_id = $1 AND deleted_at IS NULL ORDER BY created_at DESC",
    )
    .bind(listing_id)
    .fetch_all(&mut **tx)
    .await?;
    Ok(promos)
}

pub async fn find_promo_by_id(tx: &mut Transaction<'_, Postgres>, id: Uuid) -> Result<Option<StorePromo>, AppError> {
    let promo = sqlx::query_as::<_, StorePromo>("SELECT * FROM store_promos WHERE id = $1 AND deleted_at IS NULL")
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?;
    Ok(promo)
}

/// Kode promo 1 listing (tidak peduli huruf besar/kecil). `FOR UPDATE` — dipakai saat checkout
/// supaya kuota tidak terlampaui oleh 2 checkout bersamaan.
pub async fn find_promo_by_code_for_update(
    tx: &mut Transaction<'_, Postgres>,
    listing_id: Uuid,
    code: &str,
) -> Result<Option<StorePromo>, AppError> {
    let promo = sqlx::query_as::<_, StorePromo>(
        "SELECT * FROM store_promos WHERE listing_id = $1 AND upper(code) = upper($2) AND deleted_at IS NULL FOR UPDATE",
    )
    .bind(listing_id)
    .bind(code)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(promo)
}

pub struct PromoFields<'a> {
    pub code: &'a str,
    pub discount_percent: Decimal,
    pub starts_at: DateTime<Utc>,
    pub ends_at: Option<DateTime<Utc>>,
    pub max_redemptions: Option<i32>,
    pub is_active: bool,
}

pub async fn create_promo(
    tx: &mut Transaction<'_, Postgres>,
    listing_id: Uuid,
    fields: &PromoFields<'_>,
    actor: &str,
) -> Result<StorePromo, AppError> {
    let promo = sqlx::query_as::<_, StorePromo>(
        r#"
        INSERT INTO store_promos
            (listing_id, code, discount_percent, starts_at, ends_at, max_redemptions, is_active, created_by, updated_by)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $8)
        RETURNING *
        "#,
    )
    .bind(listing_id)
    .bind(fields.code)
    .bind(fields.discount_percent)
    .bind(fields.starts_at)
    .bind(fields.ends_at)
    .bind(fields.max_redemptions)
    .bind(fields.is_active)
    .bind(actor)
    .fetch_one(&mut **tx)
    .await?;
    Ok(promo)
}

pub async fn update_promo(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    fields: &PromoFields<'_>,
    actor: &str,
) -> Result<StorePromo, AppError> {
    let promo = sqlx::query_as::<_, StorePromo>(
        r#"
        UPDATE store_promos
        SET code = $2, discount_percent = $3, starts_at = $4, ends_at = $5, max_redemptions = $6,
            is_active = $7, updated_by = $8, updated_at = now()
        WHERE id = $1 AND deleted_at IS NULL
        RETURNING *
        "#,
    )
    .bind(id)
    .bind(fields.code)
    .bind(fields.discount_percent)
    .bind(fields.starts_at)
    .bind(fields.ends_at)
    .bind(fields.max_redemptions)
    .bind(fields.is_active)
    .bind(actor)
    .fetch_one(&mut **tx)
    .await?;
    Ok(promo)
}

pub async fn increment_redeemed(tx: &mut Transaction<'_, Postgres>, id: Uuid) -> Result<(), AppError> {
    sqlx::query("UPDATE store_promos SET redeemed_count = redeemed_count + 1, updated_at = now() WHERE id = $1")
        .bind(id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

pub async fn soft_delete_promo(tx: &mut Transaction<'_, Postgres>, id: Uuid, actor: &str) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE store_promos SET deleted_at = now(), deleted_by = $2, updated_at = now(), updated_by = $2 WHERE id = $1",
    )
    .bind(id)
    .bind(actor)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
