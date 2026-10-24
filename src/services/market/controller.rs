use std::sync::Arc;

use axum::{
    extract::{Query, State},
    response::Response,
    Extension,
};
use sqlx::PgPool;

use super::service::{get_klines_service, get_recommendations_service, get_symbols_service};
use crate::websocket::TickerHub;
use super::types::{
    KlinesQuery, RecommendationQuery, SymbolsQuery,
};
use crate::database::RedisPool;
use crate::utils::api_response::{paginated, success, PaginationParams};
use crate::utils::app_error::AppError;

pub async fn get_klines(
    Extension(redis): Extension<RedisPool>,
    Query(query): Query<KlinesQuery>,
) -> Result<Response, AppError> {
    let result = get_klines_service(&redis, query).await?;
    Ok(success(result, "Berhasil mengambil data chart"))
}

pub async fn get_symbols(
    State(pool): State<PgPool>,
    Extension(ticker_hub): Extension<Arc<TickerHub>>,
    Query(query): Query<SymbolsQuery>,
) -> Result<Response, AppError> {
    let pagination = PaginationParams::parse(query.page, query.limit);
    let offset = (pagination.page - 1) * pagination.limit;

    let (symbols, total) = get_symbols_service(
        &pool,
        &ticker_hub,
        query.search.as_deref(),
        pagination.limit,
        offset,
    )
    .await?;
    Ok(paginated(symbols, total, &pagination, "Berhasil mengambil daftar pair"))
}

/// Daftar pantauan coin dari API strategi Python (bukan sinyal bot, tidak dieksekusi otomatis).
/// Contoh: GET /api/market/recommendations?limit=10
pub async fn get_recommendations(
    Extension(redis): Extension<RedisPool>,
    Query(query): Query<RecommendationQuery>,
) -> Result<Response, AppError> {
    let result = get_recommendations_service(&redis, query).await?;
    Ok(success(result, "Berhasil mengambil rekomendasi coin"))
}
