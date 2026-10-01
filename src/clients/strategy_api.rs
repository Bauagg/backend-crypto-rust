//! Klien API strategi Python (`backtes-crypto`) — "otak" trading: menghitung sinyal & rekomendasi
//! dari data yang diisi backend ini ke DB. Kontrak API-nya: `backtes-crypto/docs/API_RUST.md`.
//! Respons diteruskan apa adanya (`serde_json::Value`) — bentuknya milik Python, jadi perubahan
//! field di sana tidak perlu ikut diubah di sini.

use serde_json::Value;

use crate::utils::app_error::AppError;
use crate::utils::http_client::{get_json_with_error_body, ErrorResponse};

fn api_base_url() -> String {
    std::env::var("STRATEGY_API_BASE_URL").unwrap_or_else(|_| "http://localhost:8080".to_string())
}

/// Error dari Python selalu `{"detail": ...}` — `detail` berupa string, atau list objek validasi
/// FastAPI (422). Pesan string diteruskan ke user apa adanya (mis. "Tidak ada candle untuk
/// 2030-01-01 di database"); 5xx/tidak terbaca jadi pesan umum.
fn to_app_error(error: ErrorResponse) -> AppError {
    let detail = serde_json::from_str::<Value>(&error.body)
        .ok()
        .and_then(|body| body.get("detail")?.as_str().map(str::to_string));

    match (error.status, detail) {
        (404, Some(detail)) => AppError::NotFound(detail),
        (400..=499, Some(detail)) => AppError::BadRequest(detail),
        (400..=499, None) => AppError::BadRequest("Parameter rekomendasi tidak valid".to_string()),
        (status, _) => {
            tracing::error!("API strategi error {status}: {}", error.body);
            AppError::Internal("Layanan rekomendasi sedang bermasalah".to_string())
        }
    }
}

/// `GET /recommendations/momentum` — daftar pantauan 5-10 coin (bukan sinyal bot). `date` opsional
/// (`YYYY-MM-DD`); tanpa `date` = berdasarkan candle harian terakhir.
pub async fn get_momentum_recommendations(limit: u8, date: Option<&str>) -> Result<Value, AppError> {
    let mut url = format!(
        "{}/recommendations/momentum?limit={limit}",
        api_base_url().trim_end_matches('/')
    );
    if let Some(date) = date {
        url.push_str(&format!("&date={date}"));
    }

    match get_json_with_error_body(&url).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(to_app_error(error)),
        Err(_) => Err(AppError::Internal(
            "Layanan rekomendasi sedang tidak bisa dihubungi".to_string(),
        )),
    }
}
