//! Kode promo Store — dibuat & dikelola pemilik listing. Dipakai pembeli saat checkout
//! (lihat `store_purchases`).

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use super::model::StorePromo;
use super::repository::{create_promo, find_promo_by_id, find_promos_of_listing, soft_delete_promo, update_promo, PromoFields};
use super::types::{CreatePromoInput, UpdatePromoInput};
use crate::database::{begin, commit, Tx};
use crate::services::store_listings::repository::find_listing_by_id;
use crate::utils::app_error::AppError;

fn not_found() -> AppError {
    AppError::NotFound("Promo tidak ditemukan".to_string())
}

fn bad(message: impl Into<String>) -> AppError {
    AppError::BadRequest(message.into())
}

fn normalize_code(code: &str) -> Result<String, AppError> {
    let code = code.trim().to_uppercase();
    let valid_chars = code.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !(3..=30).contains(&code.len()) || !valid_chars {
        return Err(bad("Kode promo 3–30 karakter, hanya huruf, angka, '-' atau '_'"));
    }
    Ok(code)
}

fn validate(
    discount: Decimal,
    starts_at: DateTime<Utc>,
    ends_at: Option<DateTime<Utc>>,
    max_redemptions: Option<i32>,
) -> Result<(), AppError> {
    if discount <= Decimal::ZERO || discount > Decimal::ONE_HUNDRED {
        return Err(bad("discount_percent harus 1–100"));
    }
    if ends_at.is_some_and(|end| end <= starts_at) {
        return Err(bad("ends_at harus setelah starts_at"));
    }
    if max_redemptions.is_some_and(|max| max < 1) {
        return Err(bad("max_redemptions minimal 1"));
    }
    Ok(())
}

/// Pastikan listing milik `user_id`.
async fn ensure_listing_owner(tx: &mut Tx<'_>, listing_id: Uuid, user_id: Uuid) -> Result<(), AppError> {
    find_listing_by_id(tx, listing_id)
        .await?
        .filter(|l| l.seller_user_id == user_id)
        .map(|_| ())
        .ok_or_else(|| AppError::NotFound("Listing Store tidak ditemukan".to_string()))
}

/// Promo milik listing `user_id`.
async fn owned_promo(tx: &mut Tx<'_>, id: Uuid, user_id: Uuid) -> Result<StorePromo, AppError> {
    let promo = find_promo_by_id(tx, id).await?.ok_or_else(not_found)?;
    ensure_listing_owner(tx, promo.listing_id, user_id).await.map_err(|_| not_found())?;
    Ok(promo)
}

pub async fn list_promos_service(pool: &PgPool, listing_id: Uuid, user_id: Uuid) -> Result<Vec<StorePromo>, AppError> {
    let mut tx = begin(pool).await?;
    ensure_listing_owner(&mut tx, listing_id, user_id).await?;
    let promos = find_promos_of_listing(&mut tx, listing_id).await?;
    commit(tx).await?;
    Ok(promos)
}

pub async fn create_promo_service(
    pool: &PgPool,
    user_id: Uuid,
    actor: &str,
    input: CreatePromoInput,
) -> Result<StorePromo, AppError> {
    let code = normalize_code(&input.code)?;
    let starts_at = input.starts_at.unwrap_or_else(Utc::now);
    validate(input.discount_percent, starts_at, input.ends_at, input.max_redemptions)?;

    let mut tx = begin(pool).await?;
    ensure_listing_owner(&mut tx, input.listing_id, user_id).await?;
    let fields = PromoFields {
        code: &code,
        discount_percent: input.discount_percent.round_dp(2),
        starts_at,
        ends_at: input.ends_at,
        max_redemptions: input.max_redemptions,
        is_active: true,
    };
    let promo = create_promo(&mut tx, input.listing_id, &fields, actor).await.map_err(|err| match err {
        AppError::Conflict(_) => AppError::Conflict(format!("Kode promo {code} sudah dipakai di listing ini")),
        other => other,
    })?;
    commit(tx).await?;
    Ok(promo)
}

pub async fn update_promo_service(
    pool: &PgPool,
    id: Uuid,
    user_id: Uuid,
    actor: &str,
    input: UpdatePromoInput,
) -> Result<StorePromo, AppError> {
    let mut tx = begin(pool).await?;
    let existing = owned_promo(&mut tx, id, user_id).await?;
    let code = match input.code.as_deref() {
        Some(code) => normalize_code(code)?,
        None => existing.code.clone(),
    };
    let discount = input.discount_percent.unwrap_or(existing.discount_percent);
    let starts_at = input.starts_at.unwrap_or(existing.starts_at);
    let ends_at = if input.clear_ends_at { None } else { input.ends_at.or(existing.ends_at) };
    let max_redemptions =
        if input.clear_max_redemptions { None } else { input.max_redemptions.or(existing.max_redemptions) };
    validate(discount, starts_at, ends_at, max_redemptions)?;

    let fields = PromoFields {
        code: &code,
        discount_percent: discount.round_dp(2),
        starts_at,
        ends_at,
        max_redemptions,
        is_active: input.is_active.unwrap_or(existing.is_active),
    };
    let promo = update_promo(&mut tx, id, &fields, actor).await.map_err(|err| match err {
        AppError::Conflict(_) => AppError::Conflict(format!("Kode promo {code} sudah dipakai di listing ini")),
        other => other,
    })?;
    commit(tx).await?;
    Ok(promo)
}

/// Hapus promo. Pembelian yang sudah memakainya tetap tercatat.
pub async fn delete_promo_service(pool: &PgPool, id: Uuid, user_id: Uuid, actor: &str) -> Result<(), AppError> {
    let mut tx = begin(pool).await?;
    owned_promo(&mut tx, id, user_id).await?;
    soft_delete_promo(&mut tx, id, actor).await?;
    commit(tx).await
}
