use axum::{extract::{Query, State}, response::Response};
use chrono::{Duration, NaiveDate, Utc};
use serde::Deserialize;
use sqlx::PgPool;

use super::service::get_fng_between_service;
use crate::utils::api_response::success;
use crate::utils::app_error::AppError;

#[derive(Debug, Default, Deserialize)]
pub struct FngQuery {
    /// YYYY-MM-DD; default 90 hari ke belakang dari hari ini.
    pub start: Option<String>,
    /// YYYY-MM-DD; default hari ini.
    pub end: Option<String>,
}

/// Histori Fear & Greed Index (sentimen pasar crypto, 0-100) dari cache lokal kita.
pub async fn get_fng(
    State(pool): State<PgPool>,
    Query(query): Query<FngQuery>,
) -> Result<Response, AppError> {
    let today = Utc::now().date_naive();
    let end = query
        .end
        .and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok())
        .unwrap_or(today);
    let start = query
        .start
        .and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok())
        .unwrap_or_else(|| end - Duration::days(90));

    let points = get_fng_between_service(&pool, start, end).await?;
    Ok(success(points, "Berhasil mengambil histori Fear & Greed Index"))
}
