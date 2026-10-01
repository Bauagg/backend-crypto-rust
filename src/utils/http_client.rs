use std::sync::OnceLock;
use std::time::Duration;

use reqwest::Method;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::utils::app_error::AppError;

/// Header tambahan untuk request ke API pihak ketiga, mis. API key exchange
/// (`X-MBX-APIKEY` untuk Binance, signature, dsb).
pub type Headers = Vec<(&'static str, String)>;

/// Satu client dipakai bersama seluruh aplikasi — koneksi (termasuk handshake TLS) ke host yang
/// sama dipakai ulang antar request. Membuat client baru per request berarti handshake ulang
/// tiap kali (±0,5–1 detik ke exchange luar negeri), yang bikin worker ratusan request lambat.
fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Gagal membuat HTTP client")
    })
}

async fn send<T: DeserializeOwned>(
    method: Method,
    url: &str,
    headers: Headers,
    body: Option<&(impl Serialize + ?Sized)>,
) -> Result<T, AppError> {
    let mut request = client().request(method, url);

    for (key, value) in headers {
        request = request.header(key, value);
    }

    if let Some(body) = body {
        request = request.json(body);
    }

    let response = request.send().await.map_err(|err| {
        tracing::error!("Gagal menghubungi {url}: {err}");
        AppError::Internal("Gagal menghubungi layanan eksternal".to_string())
    })?;

    if !response.status().is_success() {
        let status = response.status();
        return Err(AppError::BadRequest(format!(
            "Layanan eksternal menolak permintaan (status {status})"
        )));
    }

    response
        .json::<T>()
        .await
        .map_err(|_| AppError::Internal("Gagal membaca respons layanan eksternal".to_string()))
}

/// GET ke API pihak ketiga mana pun (Binance, Bybit, dll) — hasil JSON di-deserialize ke tipe `T`.
pub async fn get_json<T: DeserializeOwned>(url: &str) -> Result<T, AppError> {
    send::<T>(Method::GET, url, Vec::new(), Option::<&()>::None).await
}

/// Respons non-2xx dari layanan internal milik kita sendiri: status + body mentah.
pub struct ErrorResponse {
    pub status: u16,
    pub body: String,
}

/// Seperti `get_json`, tapi respons non-2xx dikembalikan utuh (status + body) di `Ok(Err(..))`,
/// tidak disamarkan jadi pesan umum — untuk layanan internal (mis. API strategi Python) yang pesan
/// error-nya memang ditujukan untuk user. Jangan dipakai untuk API pihak ketiga.
pub async fn get_json_with_error_body<T: DeserializeOwned>(
    url: &str,
) -> Result<Result<T, ErrorResponse>, AppError> {
    let response = client().get(url).send().await.map_err(|err| {
        tracing::error!("Gagal menghubungi {url}: {err}");
        AppError::Internal("Gagal menghubungi layanan eksternal".to_string())
    })?;

    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Ok(Err(ErrorResponse {
            status: status.as_u16(),
            body,
        }));
    }

    response
        .json::<T>()
        .await
        .map(Ok)
        .map_err(|_| AppError::Internal("Gagal membaca respons layanan eksternal".to_string()))
}

/// Unduh file mentah (mis. gambar) — balik bytes + `Content-Type` dari respons, ditolak kalau
/// lebih besar dari `max_bytes` supaya URL nyasar tidak menghabiskan memori.
pub async fn get_bytes(url: &str, max_bytes: usize) -> Result<(Vec<u8>, String), AppError> {
    let response = client().get(url).send().await.map_err(|err| {
        tracing::error!("Gagal menghubungi {url}: {err}");
        AppError::Internal("Gagal menghubungi layanan eksternal".to_string())
    })?;

    if !response.status().is_success() {
        let status = response.status();
        return Err(AppError::BadRequest(format!(
            "Layanan eksternal menolak permintaan (status {status})"
        )));
    }

    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/octet-stream")
        .to_string();

    let bytes = response
        .bytes()
        .await
        .map_err(|_| AppError::Internal("Gagal membaca respons layanan eksternal".to_string()))?;
    if bytes.len() > max_bytes {
        return Err(AppError::BadRequest(format!(
            "File terlalu besar ({} byte, maksimal {max_bytes})",
            bytes.len()
        )));
    }

    Ok((bytes.to_vec(), content_type))
}

/// GET dengan header tambahan (mis. API key exchange di `X-MBX-APIKEY`).
pub async fn get_json_with_headers<T: DeserializeOwned>(
    url: &str,
    headers: Headers,
) -> Result<T, AppError> {
    send::<T>(Method::GET, url, headers, Option::<&()>::None).await
}

/// POST dengan body JSON, hasil JSON di-deserialize ke tipe `T`.
#[allow(dead_code)]
pub async fn post_json<T: DeserializeOwned>(
    url: &str,
    headers: Headers,
    body: &(impl Serialize + ?Sized),
) -> Result<T, AppError> {
    send::<T>(Method::POST, url, headers, Some(body)).await
}

/// PUT dengan body JSON, hasil JSON di-deserialize ke tipe `T`.
#[allow(dead_code)]
pub async fn put_json<T: DeserializeOwned>(
    url: &str,
    headers: Headers,
    body: &(impl Serialize + ?Sized),
) -> Result<T, AppError> {
    send::<T>(Method::PUT, url, headers, Some(body)).await
}

/// PATCH dengan body JSON, hasil JSON di-deserialize ke tipe `T`.
#[allow(dead_code)]
pub async fn patch_json<T: DeserializeOwned>(
    url: &str,
    headers: Headers,
    body: &(impl Serialize + ?Sized),
) -> Result<T, AppError> {
    send::<T>(Method::PATCH, url, headers, Some(body)).await
}

/// DELETE, hasil JSON di-deserialize ke tipe `T`.
#[allow(dead_code)]
pub async fn delete_json<T: DeserializeOwned>(url: &str, headers: Headers) -> Result<T, AppError> {
    send::<T>(Method::DELETE, url, headers, Option::<&()>::None).await
}
