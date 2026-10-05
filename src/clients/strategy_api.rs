//! Klien API strategi Python (`backtes-crypto`) — "otak" trading: menghitung sinyal & rekomendasi
//! dari data yang diisi backend ini ke DB. Kontrak API-nya: `backtes-crypto/docs/API_RUST.md`.
//! Respons diteruskan apa adanya (`serde_json::Value`) — bentuknya milik Python, jadi perubahan
//! field di sana tidak perlu ikut diubah di sini.

use serde_json::Value;

use crate::utils::app_error::AppError;
use crate::utils::http_client::{get_json_with_error_body, post_json_with_error_body, ErrorResponse};

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

/// Body `POST /signal` — bentuk tetap (API_RUST.md 4.2). Rust hanya mengisi data akun apa adanya,
/// Python yang memutuskan strategi, beli/jual & kill switch.
#[derive(Debug, serde::Serialize)]
pub struct SignalRequest {
    /// Total nilai akun dalam Rupiah (coin + USDT).
    pub modal: f64,
    /// Modal saat robot mulai (Rupiah) — dasar kill switch.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modal_awal: Option<f64>,
    pub kurs_usdt_idr: f64,
    /// Jumlah coin (qty) per simbol, bukan nilai.
    pub posisi: std::collections::HashMap<String, f64>,
    /// USDT bebas.
    pub cash_usdt: f64,
}

#[derive(Debug, serde::Deserialize)]
pub struct KillSwitch {
    pub aktif: bool,
}

/// 1 order dari `/signal`, sudah diurutkan Python: semua SELL dulu, baru BUY.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct SignalOrder {
    pub urutan: u32,
    pub symbol: String,
    /// `BUY` | `SELL`
    pub side: String,
    pub nilai_usdt: f64,
    /// `true` = jual seluruh saldo coin ini.
    #[serde(default)]
    pub jual_semua: bool,
}

#[derive(Debug, serde::Deserialize)]
pub struct SignalResponse {
    /// Tanggal candle yang dipakai (`YYYY-MM-DD`) — harus = kemarin (UTC) supaya boleh trading.
    pub tanggal_candle: String,
    /// `BTC-60` | `V23`
    pub strategi: String,
    /// `RISK_ON` | `RISK_OFF` | `STOP`
    pub status: String,
    pub kill_switch: KillSwitch,
    pub perlu_rebalance: bool,
    pub order: Option<Vec<SignalOrder>>,
    #[serde(default)]
    pub catatan: Vec<String>,
}

/// `POST /signal` — sinyal harian bot trading untuk 1 akun.
pub async fn post_signal(body: &SignalRequest) -> Result<SignalResponse, AppError> {
    let url = format!("{}/signal", api_base_url().trim_end_matches('/'));
    match post_json_with_error_body(&url, body).await {
        Ok(Ok(response)) => Ok(response),
        Ok(Err(error)) => Err(to_app_error(error)),
        Err(_) => Err(AppError::Internal(
            "Layanan strategi (Python) sedang tidak bisa dihubungi".to_string(),
        )),
    }
}
