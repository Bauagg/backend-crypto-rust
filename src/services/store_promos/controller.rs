use axum::{
    extract::{Path, Query, State},
    response::Response,
    Extension, Json,
};
use sqlx::PgPool;
use uuid::Uuid;

use super::service::{create_promo_service, delete_promo_service, list_promos_service, update_promo_service};
use super::types::{CreatePromoInput, PromoQuery, UpdatePromoInput};
use crate::utils::api_response::{created, success};
use crate::utils::app_error::AppError;
use crate::utils::jwt::JwtClaims;

/// Promo 1 listing milik penjual. Contoh: GET /api/store/promos?listing_id=<uuid>
pub async fn list_promos(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Query(query): Query<PromoQuery>,
) -> Result<Response, AppError> {
    let promos = list_promos_service(&pool, query.listing_id, claims.user_uuid()?).await?;
    Ok(success(promos, "Berhasil mengambil promo"))
}

/// Body `{ "listing_id", "code", "discount_percent", "starts_at"?, "ends_at"?, "max_redemptions"? }`.
/// Contoh: POST /api/store/promos
pub async fn create_promo(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Json(input): Json<CreatePromoInput>,
) -> Result<Response, AppError> {
    let promo = create_promo_service(&pool, claims.user_uuid()?, &claims.email, input).await?;
    Ok(created(promo, "Promo berhasil dibuat"))
}

pub async fn update_promo(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdatePromoInput>,
) -> Result<Response, AppError> {
    let promo = update_promo_service(&pool, id, claims.user_uuid()?, &claims.email, input).await?;
    Ok(success(promo, "Promo berhasil diubah"))
}

pub async fn delete_promo(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Path(id): Path<Uuid>,
) -> Result<Response, AppError> {
    delete_promo_service(&pool, id, claims.user_uuid()?, &claims.email).await?;
    Ok(success(serde_json::Value::Null, "Promo berhasil dihapus"))
}
