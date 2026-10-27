use axum::{
    extract::{Path, Query, State},
    response::Response,
    Extension, Json,
};
use sqlx::PgPool;
use uuid::Uuid;

use super::service::{
    create_subscription_service, delete_subscription_service, get_subscription_performance_service,
    get_subscription_results_service, get_subscriptions_service, update_subscription_service,
};
use super::types::{CreateSubscriptionInput, UpdateSubscriptionInput};
use crate::services::strategies::types::RunQuery;
use crate::utils::api_response::{created, success};
use crate::utils::app_error::AppError;
use crate::utils::jwt::JwtClaims;

/// Strategi yang sedang dipakai user. Contoh: GET /api/strategy-subscriptions
pub async fn get_subscriptions(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
) -> Result<Response, AppError> {
    let subscriptions = get_subscriptions_service(&pool, claims.user_uuid()?).await?;
    Ok(success(subscriptions, "Berhasil mengambil strategi yang dipakai"))
}

/// Pakai strategi. Body `{ "strategy_id", "usage"?: "SCREENER" | "BOT", "account_mode"?: "DEMO" |
/// "LIVE" (BOT), "capital_percent"?: 1–100 (BOT), "input_values"? }`.
/// Contoh: POST /api/strategy-subscriptions
pub async fn create_subscription(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Json(input): Json<CreateSubscriptionInput>,
) -> Result<Response, AppError> {
    let subscription = create_subscription_service(&pool, claims.user_uuid()?, &claims.email, input).await?;
    Ok(created(subscription, "Strategi berhasil dipakai"))
}

/// Body `{ "input_values"?, "is_active"?, "capital_percent"?, "upgrade_to_latest"? }`.
pub async fn update_subscription(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Path(id): Path<Uuid>,
    Json(input): Json<UpdateSubscriptionInput>,
) -> Result<Response, AppError> {
    let subscription = update_subscription_service(&pool, id, claims.user_uuid()?, &claims.email, input).await?;
    Ok(success(subscription, "Pemakaian strategi berhasil diubah"))
}

pub async fn delete_subscription(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Path(id): Path<Uuid>,
) -> Result<Response, AppError> {
    delete_subscription_service(&pool, id, claims.user_uuid()?, &claims.email).await?;
    Ok(success(serde_json::Value::Null, "Strategi berhenti dipakai"))
}

/// Contoh: GET /api/strategy-subscriptions/:id/results?limit=10
pub async fn get_subscription_results(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Path(id): Path<Uuid>,
    Query(query): Query<RunQuery>,
) -> Result<Response, AppError> {
    let result = get_subscription_results_service(&pool, id, claims.user_uuid()?, query).await?;
    Ok(success(result, "Berhasil menjalankan strategi"))
}

/// Performa asli bot (win rate, untung/rugi terealisasi, posisi terbuka). Contoh:
/// GET /api/strategy-subscriptions/:id/performance
pub async fn get_subscription_performance(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Path(id): Path<Uuid>,
) -> Result<Response, AppError> {
    let performance = get_subscription_performance_service(&pool, id, claims.user_uuid()?).await?;
    Ok(success(performance, "Berhasil mengambil performa bot"))
}
