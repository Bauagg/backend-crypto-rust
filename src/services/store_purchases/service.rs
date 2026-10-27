//! Checkout Store — hak pakai strategi milik user lain (beli / trial).
//!
//! Pembayaran saat ini masih dummy: harga & potongan promo dicatat apa adanya, tetapi yang dibayar
//! Rp0 (`payment_status` DUMMY; FREE bila harganya memang Rp0). Saat payment gateway aktif cukup
//! mengganti langkah bayar di `checkout_service`.
//!
//! Aturan:
//! - pemilik tidak bisa membeli strateginya sendiri;
//! - FREE / ONE_TIME berlaku selamanya — tidak bisa dibeli dua kali;
//! - MONTHLY 30 hari; membeli lagi saat masih aktif = perpanjang dari tanggal habisnya;
//! - trial sekali per user per listing, hanya bila belum punya hak pakai.

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use super::model::StorePurchase;
use super::repository::{create_purchase, find_active_purchase, find_purchases_of_user, has_used_trial, NewPurchase};
use super::types::{CheckoutInput, PurchaseView, Quote};
use crate::database::{begin, commit, Tx};
use crate::services::store_listings::model::StoreListing;
use crate::services::store_listings::repository::{find_listing_by_id, find_listing_for_update, find_listings_by_ids};
use crate::services::store_promos::model::StorePromo;
use crate::services::store_promos::repository::{find_promo_by_code_for_update, increment_redeemed};
use crate::services::strategies::repository::find_strategy_by_id;
use crate::utils::app_error::AppError;

const MONTHLY_DAYS: i64 = 30;

fn bad(message: impl Into<String>) -> AppError {
    AppError::BadRequest(message.into())
}

fn listing_not_found() -> AppError {
    AppError::NotFound("Listing Store tidak ditemukan".to_string())
}

/// Listing yang bisa dibeli: tampil di Store, strateginya ACTIVE, dan bukan milik pembeli.
async fn purchasable_listing(tx: &mut Tx<'_>, listing: Option<StoreListing>, user_id: Uuid) -> Result<StoreListing, AppError> {
    let listing = listing.filter(|l| l.status == "ACTIVE").ok_or_else(listing_not_found)?;
    let strategy_active = find_strategy_by_id(tx, listing.strategy_id).await?.is_some_and(|s| s.status == "ACTIVE");
    if !strategy_active {
        return Err(listing_not_found());
    }
    if listing.seller_user_id == user_id {
        return Err(bad("Ini strategi milikmu sendiri — langsung pakai tanpa membeli"));
    }
    Ok(listing)
}

/// Promo yang sah untuk listing ini (dikunci `FOR UPDATE` sampai transaksi selesai).
async fn usable_promo(tx: &mut Tx<'_>, listing: &StoreListing, code: Option<&str>) -> Result<Option<StorePromo>, AppError> {
    let Some(code) = code.map(str::trim).filter(|c| !c.is_empty()) else { return Ok(None) };
    if listing.pricing_model == "FREE" {
        return Err(bad("Listing gratis tidak memakai kode promo"));
    }
    match find_promo_by_code_for_update(tx, listing.id, code).await? {
        Some(promo) if promo.usable_now() => Ok(Some(promo)),
        Some(_) => Err(bad("Kode promo sudah tidak berlaku (habis masa / kuota)")),
        None => Err(bad("Kode promo tidak ditemukan untuk listing ini")),
    }
}

/// Hitung rincian harga. Trial selalu Rp0 & tanpa promo.
fn quote(listing: &StoreListing, trial: bool, promo: Option<&StorePromo>) -> Quote {
    if trial {
        return Quote {
            listing_id: listing.id,
            kind: "TRIAL",
            pricing_model: listing.pricing_model.clone(),
            list_price_idr: Decimal::ZERO,
            discount_percent: Decimal::ZERO,
            discount_idr: Decimal::ZERO,
            final_price_idr: Decimal::ZERO,
            pay_now_idr: Decimal::ZERO,
            duration_days: Some(i64::from(listing.trial_days)),
            promo_code: None,
        };
    }
    let percent = promo.map(|p| p.discount_percent).unwrap_or_default();
    let discount = (listing.price_idr * percent / Decimal::ONE_HUNDRED).round_dp(2);
    Quote {
        listing_id: listing.id,
        kind: "PURCHASE",
        pricing_model: listing.pricing_model.clone(),
        list_price_idr: listing.price_idr,
        discount_percent: percent,
        discount_idr: discount,
        final_price_idr: listing.price_idr - discount,
        pay_now_idr: Decimal::ZERO,
        duration_days: (listing.pricing_model == "MONTHLY").then_some(MONTHLY_DAYS),
        promo_code: promo.map(|p| p.code.clone()),
    }
}

/// Cek apakah user boleh checkout, lalu balik masa berlaku hak pakai baru (`None` = selamanya).
async fn new_expiry(
    tx: &mut Tx<'_>,
    listing: &StoreListing,
    user_id: Uuid,
    trial: bool,
    now: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>, AppError> {
    let current = find_active_purchase(tx, user_id, listing.strategy_id).await?;
    if trial {
        if listing.trial_days == 0 {
            return Err(bad("Listing ini tidak menyediakan trial"));
        }
        if current.is_some() {
            return Err(AppError::Conflict("Kamu sudah punya hak pakai strategi ini".to_string()));
        }
        if has_used_trial(tx, user_id, listing.id).await? {
            return Err(AppError::Conflict("Trial listing ini sudah pernah kamu pakai".to_string()));
        }
        return Ok(Some(now + Duration::days(i64::from(listing.trial_days))));
    }
    match (listing.pricing_model.as_str(), current) {
        // Hak pakai selamanya tidak perlu dibeli lagi.
        (_, Some(p)) if p.kind == "PURCHASE" && p.expires_at.is_none() => {
            Err(AppError::Conflict("Kamu sudah memiliki strategi ini".to_string()))
        }
        // Perpanjang langganan: lanjut dari tanggal habis langganan yang masih jalan.
        ("MONTHLY", Some(p)) if p.kind == "PURCHASE" => {
            let base = p.expires_at.unwrap_or(now).max(now);
            Ok(Some(base + Duration::days(MONTHLY_DAYS)))
        }
        ("MONTHLY", _) => Ok(Some(now + Duration::days(MONTHLY_DAYS))),
        _ => Ok(None),
    }
}

/// Rincian harga tanpa membeli. Body sama dengan checkout.
pub async fn quote_service(pool: &PgPool, user_id: Uuid, input: &CheckoutInput) -> Result<Quote, AppError> {
    let mut tx = begin(pool).await?;
    let listing = find_listing_by_id(&mut tx, input.listing_id).await?;
    let listing = purchasable_listing(&mut tx, listing, user_id).await?;
    if input.trial && input.promo_code.is_some() {
        return Err(bad("Kode promo tidak berlaku untuk trial"));
    }
    let promo = usable_promo(&mut tx, &listing, input.promo_code.as_deref()).await?;
    new_expiry(&mut tx, &listing, user_id, input.trial, Utc::now()).await?;
    commit(tx).await?;
    Ok(quote(&listing, input.trial, promo.as_ref()))
}

/// Beli / mulai trial. Saat ini dummy: Rp0 dibayar, hak pakai langsung aktif.
pub async fn checkout_service(
    pool: &PgPool,
    user_id: Uuid,
    actor: &str,
    input: CheckoutInput,
) -> Result<PurchaseView, AppError> {
    if input.trial && input.promo_code.is_some() {
        return Err(bad("Kode promo tidak berlaku untuk trial"));
    }
    let mut tx = begin(pool).await?;
    let listing = find_listing_for_update(&mut tx, input.listing_id).await?;
    let listing = purchasable_listing(&mut tx, listing, user_id).await?;
    let promo = usable_promo(&mut tx, &listing, input.promo_code.as_deref()).await?;
    let expires_at = new_expiry(&mut tx, &listing, user_id, input.trial, Utc::now()).await?;
    let price = quote(&listing, input.trial, promo.as_ref());

    let payment_status = if price.final_price_idr.is_zero() { "FREE" } else { "DUMMY" };
    let purchase = create_purchase(
        &mut tx,
        &NewPurchase {
            user_id,
            listing_id: listing.id,
            strategy_id: listing.strategy_id,
            kind: price.kind,
            pricing_model: &listing.pricing_model,
            list_price_idr: price.list_price_idr,
            discount_idr: price.discount_idr,
            paid_idr: price.pay_now_idr,
            promo_id: promo.as_ref().map(|p| p.id),
            payment_status,
            expires_at,
        },
        actor,
    )
    .await
    .map_err(|err| match err {
        AppError::Conflict(_) => AppError::Conflict("Trial listing ini sudah pernah kamu pakai".to_string()),
        other => other,
    })?;
    if let Some(promo) = &promo {
        increment_redeemed(&mut tx, promo.id).await?;
    }
    commit(tx).await?;
    Ok(PurchaseView { is_active: true, listing_title: Some(listing.title), purchase })
}

fn is_active(purchase: &StorePurchase, now: DateTime<Utc>) -> bool {
    purchase.status == "ACTIVE" && purchase.expires_at.is_none_or(|end| end > now)
}

/// Riwayat pembelian & trial user, terbaru dulu.
pub async fn my_purchases_service(pool: &PgPool, user_id: Uuid) -> Result<Vec<PurchaseView>, AppError> {
    let mut tx = begin(pool).await?;
    let purchases = find_purchases_of_user(&mut tx, user_id).await?;
    let listing_ids: Vec<Uuid> = purchases.iter().map(|p| p.listing_id).collect();
    let titles: HashMap<Uuid, String> =
        find_listings_by_ids(&mut tx, &listing_ids).await?.into_iter().map(|l| (l.id, l.title)).collect();
    commit(tx).await?;

    let now = Utc::now();
    Ok(purchases
        .into_iter()
        .map(|purchase| PurchaseView {
            is_active: is_active(&purchase, now),
            listing_title: titles.get(&purchase.listing_id).cloned(),
            purchase,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listing(pricing_model: &str, price: i64, trial_days: i16) -> StoreListing {
        let now = Utc::now();
        StoreListing {
            id: Uuid::nil(),
            strategy_id: Uuid::nil(),
            seller_user_id: Uuid::nil(),
            title: "Test".to_string(),
            description: None,
            pricing_model: pricing_model.to_string(),
            price_idr: Decimal::from(price),
            trial_days,
            status: "ACTIVE".to_string(),
            published_at: now,
            created_by: String::new(),
            updated_by: String::new(),
            created_at: now,
            updated_at: now,
        }
    }

    fn promo(percent: i64) -> StorePromo {
        let now = Utc::now();
        StorePromo {
            id: Uuid::nil(),
            listing_id: Uuid::nil(),
            code: "HEMAT".to_string(),
            discount_percent: Decimal::from(percent),
            starts_at: now,
            ends_at: None,
            max_redemptions: None,
            redeemed_count: 0,
            is_active: true,
            created_by: String::new(),
            updated_by: String::new(),
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn promo_discount_is_applied_but_nothing_paid_yet() {
        let q = quote(&listing("MONTHLY", 150_000, 0), false, Some(&promo(20)));
        assert_eq!(q.discount_idr, Decimal::from(30_000));
        assert_eq!(q.final_price_idr, Decimal::from(120_000));
        assert_eq!(q.pay_now_idr, Decimal::ZERO);
        assert_eq!(q.duration_days, Some(MONTHLY_DAYS));
    }

    #[test]
    fn trial_is_free_for_trial_days() {
        let q = quote(&listing("ONE_TIME", 500_000, 7), true, None);
        assert_eq!(q.kind, "TRIAL");
        assert_eq!(q.final_price_idr, Decimal::ZERO);
        assert_eq!(q.duration_days, Some(7));
    }

    #[test]
    fn one_time_lasts_forever() {
        let q = quote(&listing("ONE_TIME", 500_000, 0), false, None);
        assert_eq!(q.duration_days, None);
        assert_eq!(q.final_price_idr, Decimal::from(500_000));
    }
}
