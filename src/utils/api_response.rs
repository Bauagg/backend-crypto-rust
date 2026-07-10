use axum::{http::StatusCode, response::IntoResponse, response::Response, Json};
use serde::Serialize;
use serde_json::Value;

#[derive(Serialize)]
pub struct ApiResponse<T: Serialize> {
    pub status: &'static str,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<T>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub errors: Option<Value>,
}

pub fn success<T: Serialize>(data: T, message: impl Into<String>) -> Response {
    (
        StatusCode::OK,
        Json(ApiResponse {
            status: "success",
            message: message.into(),
            data: Some(data),
            errors: None,
        }),
    )
        .into_response()
}

pub fn created<T: Serialize>(data: T, message: impl Into<String>) -> Response {
    (
        StatusCode::CREATED,
        Json(ApiResponse {
            status: "success",
            message: message.into(),
            data: Some(data),
            errors: None,
        }),
    )
        .into_response()
}

#[derive(Serialize)]
pub struct PaginationMeta {
    pub page: i64,
    pub limit: i64,
    pub total: i64,
    pub total_pages: i64,
}

#[derive(Serialize)]
struct PaginatedResponse<T: Serialize> {
    status: &'static str,
    message: String,
    data: Vec<T>,
    meta: PaginationMeta,
}

pub struct PaginationParams {
    pub page: i64,
    pub limit: i64,
}

impl PaginationParams {
    pub fn parse(page: Option<i64>, limit: Option<i64>) -> Self {
        let page = page.unwrap_or(1).max(1);
        let limit = limit.unwrap_or(10).clamp(1, 100);
        Self { page, limit }
    }
}

pub fn paginated<T: Serialize>(
    data: Vec<T>,
    total: i64,
    params: &PaginationParams,
    message: impl Into<String>,
) -> Response {
    let total_pages = (total as f64 / params.limit as f64).ceil() as i64;
    (
        StatusCode::OK,
        Json(PaginatedResponse {
            status: "success",
            message: message.into(),
            data,
            meta: PaginationMeta {
                page: params.page,
                limit: params.limit,
                total,
                total_pages,
            },
        }),
    )
        .into_response()
}
