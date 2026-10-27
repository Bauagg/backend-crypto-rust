use axum::{extract::State, response::Response, Extension, Json};
use sqlx::PgPool;

use super::service::{checkout_service, my_purchases_service, quote_service};
use super::types::CheckoutInput;
use crate::utils::api_response::{created, success};
use crate::utils::app_error::AppError;
use crate::utils::jwt::JwtClaims;

/// Rincian harga (promo/trial) tanpa membeli. Body `{ "listing_id", "promo_code"?, "trial"? }`.
/// Contoh: POST /api/store/purchases/quote
pub async fn quote(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Json(input): Json<CheckoutInput>,
) -> Result<Response, AppError> {
    let quote = quote_service(&pool, claims.user_uuid()?, &input).await?;
    Ok(success(quote, "Berhasil menghitung harga"))
}

/// Beli / mulai trial (pembayaran masih dummy Rp0). Contoh: POST /api/store/purchases
pub async fn checkout(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Json(input): Json<CheckoutInput>,
) -> Result<Response, AppError> {
    let purchase = checkout_service(&pool, claims.user_uuid()?, &claims.email, input).await?;
    Ok(created(purchase, "Strategi berhasil ditambahkan ke akunmu"))
}

/// Riwayat pembelian & trial. Contoh: GET /api/store/purchases
pub async fn my_purchases(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
) -> Result<Response, AppError> {
    let purchases = my_purchases_service(&pool, claims.user_uuid()?).await?;
    Ok(success(purchases, "Berhasil mengambil pembelian"))
}
