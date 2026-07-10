use axum::{http::StatusCode, response::IntoResponse, response::Response, Json};
use serde::Serialize;
use serde_json::json;

#[derive(Debug, Clone, Serialize)]
pub struct FieldError {
    pub field: String,
    pub message: String,
}

#[derive(Debug)]
pub enum AppError {
    BadRequest(String),
    Validation(Vec<FieldError>),
    Unauthorized(String),
    /// Siap dipakai saat ada endpoint yang membatasi akses berdasar role (mis. khusus admin).
    #[allow(dead_code)]
    Forbidden(String),
    NotFound(String),
    Conflict(String),
    Internal(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        if let AppError::Validation(errors) = &self {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "status": "fail",
                    "message": "Validasi gagal",
                    "errors": errors,
                })),
            )
                .into_response();
        }

        let (status_code, message) = match &self {
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg.as_str()),
            AppError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg.as_str()),
            AppError::Forbidden(msg) => (StatusCode::FORBIDDEN, msg.as_str()),
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, msg.as_str()),
            AppError::Conflict(msg) => (StatusCode::CONFLICT, msg.as_str()),
            AppError::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg.as_str()),
            AppError::Validation(_) => unreachable!(),
        };

        let status = if status_code.as_u16() >= 500 {
            "error"
        } else {
            "fail"
        };

        if matches!(self, AppError::Internal(_)) {
            tracing::error!("{}", message);
        }

        (
            status_code,
            Json(json!({
                "status": status,
                "message": message,
            })),
        )
            .into_response()
    }
}

impl From<validator::ValidationErrors> for AppError {
    fn from(err: validator::ValidationErrors) -> Self {
        let errors = err
            .field_errors()
            .into_iter()
            .flat_map(|(field, field_errors)| {
                field_errors.iter().map(move |e| FieldError {
                    field: field.to_string(),
                    message: e
                        .message
                        .as_ref()
                        .map(|m| m.to_string())
                        .unwrap_or_else(|| format!("{field} tidak valid")),
                })
            })
            .collect();

        AppError::Validation(errors)
    }
}

impl From<sqlx::Error> for AppError {
    fn from(err: sqlx::Error) -> Self {
        match &err {
            sqlx::Error::RowNotFound => AppError::NotFound("Data tidak ditemukan".to_string()),
            sqlx::Error::Database(db_err) => {
                if db_err.is_unique_violation() {
                    AppError::Conflict("Data sudah ada".to_string())
                } else if db_err.is_foreign_key_violation() {
                    AppError::BadRequest("Referensi data tidak ditemukan".to_string())
                } else {
                    AppError::Internal("Terjadi kesalahan pada database".to_string())
                }
            }
            _ => AppError::Internal("Terjadi kesalahan pada database".to_string()),
        }
    }
}
