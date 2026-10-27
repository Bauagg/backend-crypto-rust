use rust_decimal::Decimal;
use sqlx::{Postgres, QueryBuilder, Transaction};
use uuid::Uuid;

use super::model::StoreListing;
use crate::utils::app_error::AppError;

pub async fn find_listing_by_id(tx: &mut Transaction<'_, Postgres>, id: Uuid) -> Result<Option<StoreListing>, AppError> {
    let listing = sqlx::query_as::<_, StoreListing>("SELECT * FROM store_listings WHERE id = $1 AND deleted_at IS NULL")
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?;
    Ok(listing)
}

/// Termasuk yang sudah dicabut — riwayat pembelian tetap menampilkan judulnya.
pub async fn find_listings_by_ids(tx: &mut Transaction<'_, Postgres>, ids: &[Uuid]) -> Result<Vec<StoreListing>, AppError> {
    let listings = sqlx::query_as::<_, StoreListing>("SELECT * FROM store_listings WHERE id = ANY($1)")
        .bind(ids)
        .fetch_all(&mut **tx)
        .await?;
    Ok(listings)
}

/// Kunci baris listing aktif (`FOR UPDATE`) — checkout bersamaan oleh user yang sama diproses berurutan.
pub async fn find_listing_for_update(tx: &mut Transaction<'_, Postgres>, id: Uuid) -> Result<Option<StoreListing>, AppError> {
    let listing = sqlx::query_as::<_, StoreListing>(
        "SELECT * FROM store_listings WHERE id = $1 AND deleted_at IS NULL FOR UPDATE",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(listing)
}

pub async fn find_listing_of_strategy(tx: &mut Transaction<'_, Postgres>, strategy_id: Uuid) -> Result<Option<StoreListing>, AppError> {
    let listing = sqlx::query_as::<_, StoreListing>(
        "SELECT * FROM store_listings WHERE strategy_id = $1 AND deleted_at IS NULL",
    )
    .bind(strategy_id)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(listing)
}

pub async fn find_listings_of_seller(tx: &mut Transaction<'_, Postgres>, seller_id: Uuid) -> Result<Vec<StoreListing>, AppError> {
    let listings = sqlx::query_as::<_, StoreListing>(
        "SELECT * FROM store_listings WHERE seller_user_id = $1 AND deleted_at IS NULL ORDER BY published_at DESC",
    )
    .bind(seller_id)
    .fetch_all(&mut **tx)
    .await?;
    Ok(listings)
}

/// Filter halaman Store.
pub struct BrowseFilter<'a> {
    /// Cocok dengan judul listing, nama strategi, atau nama pembuat.
    pub search: Option<&'a str>,
    /// `SCREENER` | `BOT`
    pub kind: Option<&'a str>,
    /// `FREE` | `ONE_TIME` | `MONTHLY`
    pub pricing_model: Option<&'a str>,
    /// Listing milik user ini tidak ditampilkan (dagangan sendiri ada di daftar "milik saya").
    pub exclude_seller: Option<Uuid>,
}

fn push_browse_conditions<'a>(query: &mut QueryBuilder<'a, Postgres>, filter: &BrowseFilter<'a>) {
    query.push(
        " FROM store_listings l JOIN strategies s ON s.id = l.strategy_id JOIN users u ON u.id = l.seller_user_id \
         WHERE l.status = 'ACTIVE' AND l.deleted_at IS NULL AND s.status = 'ACTIVE' AND s.deleted_at IS NULL",
    );
    if let Some(seller) = filter.exclude_seller {
        query.push(" AND l.seller_user_id <> ").push_bind(seller);
    }
    if let Some(search) = filter.search {
        query
            .push(" AND (l.title ILIKE '%' || ")
            .push_bind(search)
            .push(" || '%' OR s.name ILIKE '%' || ")
            .push_bind(search)
            .push(" || '%' OR u.full_name ILIKE '%' || ")
            .push_bind(search)
            .push(" || '%')");
    }
    if let Some(kind) = filter.kind {
        query.push(" AND s.kind = ").push_bind(kind);
    }
    if let Some(pricing) = filter.pricing_model {
        query.push(" AND l.pricing_model = ").push_bind(pricing);
    }
}

/// Listing yang tampil di Store (listing & strategi aktif), terbaru dulu. Balik `(data, total)`.
pub async fn find_active_listings(
    tx: &mut Transaction<'_, Postgres>,
    filter: &BrowseFilter<'_>,
    limit: i64,
    offset: i64,
) -> Result<(Vec<StoreListing>, i64), AppError> {
    let mut query = QueryBuilder::<Postgres>::new("SELECT l.*");
    push_browse_conditions(&mut query, filter);
    query.push(" ORDER BY l.published_at DESC LIMIT ").push_bind(limit).push(" OFFSET ").push_bind(offset);
    let listings = query.build_query_as::<StoreListing>().fetch_all(&mut **tx).await?;

    let mut count = QueryBuilder::<Postgres>::new("SELECT COUNT(*)");
    push_browse_conditions(&mut count, filter);
    let total: i64 = count.build_query_scalar().fetch_one(&mut **tx).await?;
    Ok((listings, total))
}

pub struct ListingFields<'a> {
    pub title: &'a str,
    pub description: Option<&'a str>,
    pub pricing_model: &'a str,
    pub price_idr: Decimal,
    pub trial_days: i16,
    pub status: &'a str,
}

pub async fn create_listing(
    tx: &mut Transaction<'_, Postgres>,
    strategy_id: Uuid,
    seller_id: Uuid,
    fields: &ListingFields<'_>,
    actor: &str,
) -> Result<StoreListing, AppError> {
    let listing = sqlx::query_as::<_, StoreListing>(
        r#"
        INSERT INTO store_listings
            (strategy_id, seller_user_id, title, description, pricing_model, price_idr, trial_days,
             status, created_by, updated_by)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $9)
        RETURNING *
        "#,
    )
    .bind(strategy_id)
    .bind(seller_id)
    .bind(fields.title)
    .bind(fields.description)
    .bind(fields.pricing_model)
    .bind(fields.price_idr)
    .bind(fields.trial_days)
    .bind(fields.status)
    .bind(actor)
    .fetch_one(&mut **tx)
    .await?;
    Ok(listing)
}

pub async fn update_listing(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    fields: &ListingFields<'_>,
    actor: &str,
) -> Result<StoreListing, AppError> {
    let listing = sqlx::query_as::<_, StoreListing>(
        r#"
        UPDATE store_listings
        SET title = $2, description = $3, pricing_model = $4, price_idr = $5, trial_days = $6,
            status = $7, updated_by = $8, updated_at = now()
        WHERE id = $1 AND deleted_at IS NULL
        RETURNING *
        "#,
    )
    .bind(id)
    .bind(fields.title)
    .bind(fields.description)
    .bind(fields.pricing_model)
    .bind(fields.price_idr)
    .bind(fields.trial_days)
    .bind(fields.status)
    .bind(actor)
    .fetch_one(&mut **tx)
    .await?;
    Ok(listing)
}

pub async fn soft_delete_listing(tx: &mut Transaction<'_, Postgres>, id: Uuid, actor: &str) -> Result<(), AppError> {
    sqlx::query(
        "UPDATE store_listings SET deleted_at = now(), deleted_by = $2, updated_at = now(), updated_by = $2 WHERE id = $1",
    )
    .bind(id)
    .bind(actor)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
