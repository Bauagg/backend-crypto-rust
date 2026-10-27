//! Store Indicator Bot — strategi yang dipasang pembuatnya untuk dipakai user lain (screener/bot).
//! Isi script tidak pernah dikirim ke pembeli. Pembuat ditampilkan dengan nama aslinya.
//!
//! Statistik yang tampil dihitung sistem, bukan diisi pembuat:
//! - backtest resmi (dijalankan pembuat, setelan default) versi aktif — wajib ada sebelum dipasang;
//! - performa asli semua pemakai dari `trade_positions` (DEMO & LIVE dipisah).

use std::collections::HashMap;

use chrono::Utc;
use rust_decimal::Decimal;
use sqlx::PgPool;
use uuid::Uuid;

use super::model::StoreListing;
use super::repository::{
    create_listing, find_active_listings, find_listing_by_id, find_listing_of_strategy, find_listings_of_seller,
    soft_delete_listing, update_listing, BrowseFilter, ListingFields,
};
use super::types::{
    BrowseQuery, CreateListingInput, ListedStrategy, ListingCard, MyAccess, Seller, UpdateListingInput,
};
use crate::database::{begin, commit, Tx};
use crate::services::store_purchases::repository::{count_active_buyers, find_active_purchases_for, has_used_trial};
use crate::services::strategies::model::Strategy;
use crate::services::strategies::repository::{find_strategies_by_ids, find_strategy_by_id, set_visibility};
use crate::services::strategies::service::strategy_stats_in_tx;
use crate::services::strategy_backtests::repository::find_latest_official_backtest;
use crate::services::strategy_versions::repository::find_versions_by_ids;
use crate::services::strategy_versions::types::BotConfig;
use crate::services::users::repository::find_users_by_ids;
use crate::strategy_engine;
use crate::utils::app_error::AppError;

const PRICING_MODELS: [&str; 3] = ["FREE", "ONE_TIME", "MONTHLY"];
const LISTING_STATUSES: [&str; 2] = ["ACTIVE", "INACTIVE"];
const MAX_TITLE_CHARS: usize = 120;

fn not_found() -> AppError {
    AppError::NotFound("Listing Store tidak ditemukan".to_string())
}

fn bad(message: impl Into<String>) -> AppError {
    AppError::BadRequest(message.into())
}

fn choice(value: &str, allowed: &[&str], field: &str) -> Result<String, AppError> {
    let value = value.trim().to_uppercase();
    if allowed.contains(&value.as_str()) {
        Ok(value)
    } else {
        Err(bad(format!("{field} harus salah satu dari: {}", allowed.join(", "))))
    }
}

/// Harga & trial yang sah: FREE = Rp0, ONE_TIME/MONTHLY = harga > 0; trial 0–90 hari.
fn pricing(model: &str, price: Decimal, trial_days: i16) -> Result<Decimal, AppError> {
    if !(0..=90).contains(&trial_days) {
        return Err(bad("trial_days harus 0–90"));
    }
    match model {
        "FREE" => Ok(Decimal::ZERO),
        _ if price <= Decimal::ZERO => Err(bad("price_idr wajib lebih dari 0 untuk ONE_TIME / MONTHLY")),
        _ => Ok(price.round_dp(2)),
    }
}

fn title_of(title: Option<&str>, fallback: &str) -> Result<String, AppError> {
    let title = title.map(str::trim).filter(|t| !t.is_empty()).unwrap_or(fallback).to_string();
    if title.chars().count() > MAX_TITLE_CHARS {
        return Err(bad(format!("Judul maksimal {MAX_TITLE_CHARS} karakter")));
    }
    Ok(title)
}

async fn owned_listing(tx: &mut Tx<'_>, id: Uuid, user_id: Uuid) -> Result<StoreListing, AppError> {
    find_listing_by_id(tx, id).await?.filter(|l| l.seller_user_id == user_id).ok_or_else(not_found)
}

/// Susun kartu Store (data strategi, penjual, statistik, akses user) untuk beberapa listing.
async fn cards(
    tx: &mut Tx<'_>,
    listings: Vec<StoreListing>,
    viewer_id: Uuid,
    with_curve: bool,
) -> Result<Vec<ListingCard>, AppError> {
    let strategy_ids: Vec<Uuid> = listings.iter().map(|l| l.strategy_id).collect();
    let strategies: HashMap<Uuid, Strategy> =
        find_strategies_by_ids(tx, &strategy_ids).await?.into_iter().map(|s| (s.id, s)).collect();
    let version_ids: Vec<Uuid> = strategies.values().filter_map(|s| s.current_version_id).collect();
    let versions: HashMap<Uuid, _> = find_versions_by_ids(tx, &version_ids).await?.into_iter().map(|v| (v.id, v)).collect();
    let seller_ids: Vec<Uuid> = listings.iter().map(|l| l.seller_user_id).collect();
    let sellers: HashMap<Uuid, _> = find_users_by_ids(tx, &seller_ids).await?.into_iter().map(|u| (u.id, u)).collect();
    let listing_ids: Vec<Uuid> = listings.iter().map(|l| l.id).collect();
    let buyers: HashMap<Uuid, i64> = count_active_buyers(tx, &listing_ids).await?.into_iter().collect();
    let my_purchases = find_active_purchases_for(tx, viewer_id, &strategy_ids).await?;

    let mut out = Vec::with_capacity(listings.len());
    for listing in listings {
        let Some(strategy) = strategies.get(&listing.strategy_id) else { continue };
        let version = strategy.current_version_id.and_then(|id| versions.get(&id));
        let stats = strategy_stats_in_tx(tx, strategy, viewer_id, with_curve).await?;
        let my_access = if listing.seller_user_id == viewer_id {
            Some(MyAccess { kind: "OWNER".to_string(), expires_at: None })
        } else {
            my_purchases
                .iter()
                .filter(|p| p.strategy_id == strategy.id)
                .max_by_key(|p| p.expires_at.unwrap_or(chrono::DateTime::<Utc>::MAX_UTC))
                .map(|p| MyAccess { kind: p.kind.clone(), expires_at: p.expires_at })
        };
        let trial_available = listing.trial_days > 0
            && listing.seller_user_id != viewer_id
            && !has_used_trial(tx, viewer_id, listing.id).await?;

        out.push(ListingCard {
            seller: sellers.get(&listing.seller_user_id).map(|u| Seller {
                user_id: u.id,
                full_name: u.full_name.clone(),
                photo_url: u.photo_url.clone(),
            }),
            strategy: ListedStrategy {
                id: strategy.id,
                name: strategy.name.clone(),
                kind: strategy.kind.clone(),
                version_no: version.map(|v| v.version_no),
                timeframe: strategy_engine::TIMEFRAME,
                bot: version.map(BotConfig::of),
            },
            stats,
            active_users: buyers.get(&listing.id).copied().unwrap_or_default(),
            my_access,
            trial_available,
            listing,
        });
    }
    Ok(out)
}

/// Pasang strategi milik user di Store. Syarat: strategi ACTIVE & sudah punya backtest resmi untuk
/// versi aktifnya (statistik Store tidak boleh kosong).
pub async fn create_listing_service(
    pool: &PgPool,
    user_id: Uuid,
    actor: &str,
    input: CreateListingInput,
) -> Result<ListingCard, AppError> {
    let pricing_model = choice(input.pricing_model.as_deref().unwrap_or("FREE"), &PRICING_MODELS, "pricing_model")?;
    let trial_days = input.trial_days.unwrap_or(0);
    let price = pricing(&pricing_model, input.price_idr.unwrap_or_default(), trial_days)?;

    let mut tx = begin(pool).await?;
    let strategy = find_strategy_by_id(&mut tx, input.strategy_id)
        .await?
        .filter(|s| s.user_id == user_id)
        .ok_or_else(|| AppError::NotFound("Strategi tidak ditemukan".to_string()))?;
    if strategy.status != "ACTIVE" {
        return Err(bad("Ubah status strategi ke ACTIVE dulu sebelum dipasang di Store"));
    }
    let version_id = strategy.current_version_id.ok_or_else(|| bad("Strategi belum punya versi script"))?;
    if find_latest_official_backtest(&mut tx, version_id, user_id).await?.is_none() {
        return Err(bad("Jalankan backtest versi aktif dulu — statistik Store diambil dari backtest itu"));
    }
    if find_listing_of_strategy(&mut tx, strategy.id).await?.is_some() {
        return Err(AppError::Conflict("Strategi ini sudah dipasang di Store".to_string()));
    }
    let title = title_of(input.title.as_deref(), &strategy.name)?;
    let description = input.description.as_deref().map(str::trim).filter(|d| !d.is_empty());
    let listing = create_listing(
        &mut tx,
        strategy.id,
        user_id,
        &ListingFields { title: &title, description, pricing_model: &pricing_model, price_idr: price, trial_days, status: "ACTIVE" },
        actor,
    )
    .await?;
    set_visibility(&mut tx, strategy.id, "PUBLIC", actor).await?;
    let mut details = cards(&mut tx, vec![listing], user_id, true).await?;
    commit(tx).await?;
    details.pop().ok_or_else(not_found)
}

pub async fn update_listing_service(
    pool: &PgPool,
    id: Uuid,
    user_id: Uuid,
    actor: &str,
    input: UpdateListingInput,
) -> Result<ListingCard, AppError> {
    let mut tx = begin(pool).await?;
    let existing = owned_listing(&mut tx, id, user_id).await?;
    let pricing_model = match input.pricing_model.as_deref() {
        Some(model) => choice(model, &PRICING_MODELS, "pricing_model")?,
        None => existing.pricing_model.clone(),
    };
    let trial_days = input.trial_days.unwrap_or(existing.trial_days);
    let price = pricing(&pricing_model, input.price_idr.unwrap_or(existing.price_idr), trial_days)?;
    let status = match input.status.as_deref() {
        Some(status) => choice(status, &LISTING_STATUSES, "status")?,
        None => existing.status.clone(),
    };
    let title = title_of(input.title.as_deref(), &existing.title)?;
    let description = match input.description.as_deref().map(str::trim) {
        Some("") => None,
        Some(d) => Some(d.to_string()),
        None => existing.description.clone(),
    };
    let listing = update_listing(
        &mut tx,
        id,
        &ListingFields {
            title: &title,
            description: description.as_deref(),
            pricing_model: &pricing_model,
            price_idr: price,
            trial_days,
            status: &status,
        },
        actor,
    )
    .await?;
    let mut details = cards(&mut tx, vec![listing], user_id, true).await?;
    commit(tx).await?;
    details.pop().ok_or_else(not_found)
}

/// Cabut strategi dari Store. Pembeli/trial yang masih berlaku tetap bisa memakainya sampai habis.
pub async fn delete_listing_service(pool: &PgPool, id: Uuid, user_id: Uuid, actor: &str) -> Result<(), AppError> {
    let mut tx = begin(pool).await?;
    let listing = owned_listing(&mut tx, id, user_id).await?;
    soft_delete_listing(&mut tx, id, actor).await?;
    set_visibility(&mut tx, listing.strategy_id, "PRIVATE", actor).await?;
    commit(tx).await
}

/// Halaman Store: listing aktif milik user lain, terbaru dulu. Dagangan sendiri tidak ikut (ada di
/// `my_listings_service`). Balik `(data, total)`.
pub async fn browse_listings_service(
    pool: &PgPool,
    user_id: Uuid,
    query: &BrowseQuery,
    limit: i64,
    offset: i64,
) -> Result<(Vec<ListingCard>, i64), AppError> {
    let kind = query.kind.as_deref().map(|k| k.trim().to_uppercase());
    let pricing_model = query.pricing_model.as_deref().map(|p| p.trim().to_uppercase());
    let search = query.search.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let filter = BrowseFilter {
        search,
        kind: kind.as_deref(),
        pricing_model: pricing_model.as_deref(),
        exclude_seller: Some(user_id),
    };

    let mut tx = begin(pool).await?;
    let (listings, total) = find_active_listings(&mut tx, &filter, limit, offset).await?;
    let details = cards(&mut tx, listings, user_id, false).await?;
    commit(tx).await?;
    Ok((details, total))
}

/// Listing milik user (penjual), termasuk yang INACTIVE.
pub async fn my_listings_service(pool: &PgPool, user_id: Uuid) -> Result<Vec<ListingCard>, AppError> {
    let mut tx = begin(pool).await?;
    let listings = find_listings_of_seller(&mut tx, user_id).await?;
    let details = cards(&mut tx, listings, user_id, false).await?;
    commit(tx).await?;
    Ok(details)
}

/// Detail listing + kurva modal/drawdown backtest resmi. Listing INACTIVE hanya untuk penjual &
/// pemegang hak pakai.
pub async fn get_listing_service(pool: &PgPool, id: Uuid, user_id: Uuid) -> Result<ListingCard, AppError> {
    let mut tx = begin(pool).await?;
    let listing = find_listing_by_id(&mut tx, id).await?.ok_or_else(not_found)?;
    let mut details = cards(&mut tx, vec![listing], user_id, true).await?;
    commit(tx).await?;
    let detail = details.pop().ok_or_else(not_found)?;
    if detail.listing.status != "ACTIVE" && detail.my_access.is_none() {
        return Err(not_found());
    }
    Ok(detail)
}
