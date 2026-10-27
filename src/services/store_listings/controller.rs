use axum::{
    extract::{Path, Query, State},
    response::Response,
    Extension, Json,
};
use sqlx::PgPool;
use uuid::Uuid;

use super::service::{
    browse_listings_service, create_listing_service, delete_listing_service, get_listing_service,
    my_listings_service, update_listing_service,
};
use super::types::{BrowseQuery, CreateListingInput, UpdateListingInput};
use crate::utils::api_response::{created, paginated, success, PaginationParams};
use crate::utils::app_error::AppError;
use crate::utils::jwt::JwtClaims;

/// Halaman Store. Contoh: GET /api/store/listings?page=1&limit=20&kind=BOT&search=rsi
pub async fn browse_listings(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Query(query): Query<BrowseQuery>,
) -> Result<Response, AppError> {
    let pagination = PaginationParams::parse(query.page, query.limit);
    let offset = (pagination.page - 1) * pagination.limit;
    let (listings, total) = browse_listings_service(&pool, claims.user_uuid()?, &query, pagination.limit, offset).await?;
    Ok(paginated(listings, total, &pagination, "Berhasil mengambil Store"))
}

/// Listing milik penjual yang login. Contoh: GET /api/store/listings/mine
pub async fn my_listings(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
) -> Result<Response, AppError> {
    let listings = my_listings_service(&pool, claims.user_uuid()?).await?;
    Ok(success(listings, "Berhasil mengambil listing saya"))
}

/// Pasang strategi di Store. Body `{ "strategy_id", "title"?, "description"?, "pricing_model"?,
/// "price_idr"?, "trial_days"? }`. Contoh: POST /api/store/listings
pub async fn create_listing(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Json(input): Json<CreateListingInput>,
) -> Result<Response, AppError> {
    let listing = create_listing_service(&pool, claims.user_uuid()?, &claims.email, input).await?;
    Ok(created(listing, "Strategi berhasil dipasang di Store"))
}

/// Detail listing + statistik & kurva modal/drawdown. Contoh: GET /api/store/listings/:id
pub async fn get_listing(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Path(id): Path<Uuid>,
) -> Result<Response, AppError> {
    let listing = get_listing_service(&pool, id, claims.user_uuid()?).await?;
    Ok(success(listing, "Berhasil mengambil listing"))
}

pub async fn update_listing(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateListingInput>,
) -> Result<Response, AppError> {
    let listing = update_listing_service(&pool, id, claims.user_uuid()?, &claims.email, input).await?;
    Ok(success(listing, "Listing berhasil diubah"))
}

/// Cabut dari Store. Contoh: DELETE /api/store/listings/:id
pub async fn delete_listing(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Path(id): Path<Uuid>,
) -> Result<Response, AppError> {
    delete_listing_service(&pool, id, claims.user_uuid()?, &claims.email).await?;
    Ok(success(serde_json::Value::Null, "Strategi dicabut dari Store"))
}
