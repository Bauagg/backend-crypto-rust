use std::sync::OnceLock;
use std::time::Duration;

use reqwest::Method;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::utils::app_error::AppError;

/// Header tambahan untuk request ke API pihak ketiga, mis. API key exchange
/// (`X-MBX-APIKEY` untuk Binance, signature, dsb).
pub type Headers = Vec<(&'static str, String)>;

/// Batas waktu default 1 request (termasuk mengunduh body). Respons besar yang memang butuh lebih
/// lama (mis. `exchangeInfo` Binance, belasan MB) pakai `get_json_with_timeout`.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Satu client dipakai bersama seluruh aplikasi — koneksi (termasuk handshake TLS) ke host yang
/// sama dipakai ulang antar request. Membuat client baru per request berarti handshake ulang
/// tiap kali (±0,5–1 detik ke exchange luar negeri), yang bikin worker ratusan request lambat.
fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
            .expect("Gagal membuat HTTP client")
    })
}

/// Bedakan "terlalu lama" dari "format respons tidak sesuai" — dua masalah yang perbaikannya beda.
fn read_error(url: &str, err: reqwest::Error) -> AppError {
    tracing::error!("Gagal membaca respons {url}: {err}");
    if err.is_timeout() {
        AppError::Internal("Layanan eksternal terlalu lama merespons".to_string())
    } else {
        AppError::Internal("Gagal membaca respons layanan eksternal".to_string())
    }
}

async fn send<T: DeserializeOwned>(
    method: Method,
    url: &str,
    headers: Headers,
    body: Option<&(impl Serialize + ?Sized)>,
    timeout: Duration,
) -> Result<T, AppError> {
    let mut request = client().request(method, url).timeout(timeout);

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

    response.json::<T>().await.map_err(|err| read_error(url, err))
}

/// GET ke API pihak ketiga mana pun (Binance, Bybit, dll) — hasil JSON di-deserialize ke tipe `T`.
pub async fn get_json<T: DeserializeOwned>(url: &str) -> Result<T, AppError> {
    send::<T>(Method::GET, url, Vec::new(), Option::<&()>::None, DEFAULT_TIMEOUT).await
}

/// Seperti `get_json`, dengan batas waktu sendiri — untuk respons besar yang wajar butuh waktu
/// lebih lama dari default (dijalankan di worker, bukan di request user).
pub async fn get_json_with_timeout<T: DeserializeOwned>(
    url: &str,
    timeout: Duration,
) -> Result<T, AppError> {
    send::<T>(Method::GET, url, Vec::new(), Option::<&()>::None, timeout).await
}

/// Respons non-2xx apa adanya: status + body mentah.
pub struct ErrorResponse {
    pub status: u16,
    pub body: String,
}

/// Seperti `get_json`, tapi respons non-2xx dikembalikan utuh (status + body) di `Ok(Err(..))`,
/// tidak disamarkan jadi pesan umum — untuk layanan internal (mis. API strategi Python) yang pesan
/// error-nya memang ditujukan untuk user. Jangan dipakai untuk API pihak ketiga.
pub async fn get_json_with_error_body<T: DeserializeOwned>(
    url: &str,
    headers: Headers,
) -> Result<Result<T, ErrorResponse>, AppError> {
    let mut request = client().get(url).timeout(DEFAULT_TIMEOUT);
    for (key, value) in headers {
        request = request.header(key, value);
    }
    let response = request.send().await.map_err(|err| {
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
        .map_err(|err| read_error(url, err))
}

/// POST JSON versi `get_json_with_error_body` — untuk layanan internal (API strategi Python).
/// Batas waktu 60 detik: perhitungan sinyal di Python bisa butuh beberapa detik per akun.
pub async fn post_json_with_error_body<T: DeserializeOwned>(
    url: &str,
    headers: Headers,
    body: &(impl Serialize + ?Sized),
) -> Result<Result<T, ErrorResponse>, AppError> {
    let mut request = client().post(url).timeout(Duration::from_secs(60)).json(body);
    for (key, value) in headers {
        request = request.header(key, value);
    }
    let response = request
        .send()
        .await
        .map_err(|err| {
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
        .map_err(|err| read_error(url, err))
}

/// Unduh file mentah (mis. gambar) — balik bytes + `Content-Type` dari respons, ditolak kalau
/// lebih besar dari `max_bytes` supaya URL nyasar tidak menghabiskan memori.
pub async fn get_bytes(url: &str, max_bytes: usize) -> Result<(Vec<u8>, String), AppError> {
    let response = client().get(url).timeout(DEFAULT_TIMEOUT).send().await.map_err(|err| {
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
        .map_err(|err| read_error(url, err))?;
    if bytes.len() > max_bytes {
        return Err(AppError::BadRequest(format!(
            "File terlalu besar ({} byte, maksimal {max_bytes})",
            bytes.len()
        )));
    }

    Ok((bytes.to_vec(), content_type))
}

/// GET dengan header, respons non-2xx dikembalikan utuh (status + body) di `Ok(Err(..))` — untuk
/// API pihak ketiga yang kode error-nya perlu dibedakan (mis. Binance `{"code":-2015,"msg":..}`).
/// Body-nya diterjemahkan dulu oleh pemanggil, jangan diteruskan mentah ke user.
pub async fn get_json_with_headers_error_body<T: DeserializeOwned>(
    url: &str,
    headers: Headers,
) -> Result<Result<T, ErrorResponse>, AppError> {
    send_with_error_body(Method::GET, url, headers).await
}

/// POST tanpa body (parameter di query string, mis. order Binance) — versi POST dari
/// `get_json_with_headers_error_body`.
pub async fn post_with_headers_error_body<T: DeserializeOwned>(
    url: &str,
    headers: Headers,
) -> Result<Result<T, ErrorResponse>, AppError> {
    send_with_error_body(Method::POST, url, headers).await
}

async fn send_with_error_body<T: DeserializeOwned>(
    method: Method,
    url: &str,
    headers: Headers,
) -> Result<Result<T, ErrorResponse>, AppError> {
    let mut request = client().request(method, url).timeout(DEFAULT_TIMEOUT);
    for (key, value) in headers {
        request = request.header(key, value);
    }
    let response = request.send().await.map_err(|err| {
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
        .map_err(|err| read_error(url, err))
}

/// GET dengan header tambahan (mis. API key exchange di `X-MBX-APIKEY`).
pub async fn get_json_with_headers<T: DeserializeOwned>(
    url: &str,
    headers: Headers,
) -> Result<T, AppError> {
    send::<T>(Method::GET, url, headers, Option::<&()>::None, DEFAULT_TIMEOUT).await
}

/// POST dengan body JSON, hasil JSON di-deserialize ke tipe `T`.
#[allow(dead_code)]
pub async fn post_json<T: DeserializeOwned>(
    url: &str,
    headers: Headers,
    body: &(impl Serialize + ?Sized),
) -> Result<T, AppError> {
    send::<T>(Method::POST, url, headers, Some(body), DEFAULT_TIMEOUT).await
}

/// PUT dengan body JSON, hasil JSON di-deserialize ke tipe `T`.
#[allow(dead_code)]
pub async fn put_json<T: DeserializeOwned>(
    url: &str,
    headers: Headers,
    body: &(impl Serialize + ?Sized),
) -> Result<T, AppError> {
    send::<T>(Method::PUT, url, headers, Some(body), DEFAULT_TIMEOUT).await
}

/// PATCH dengan body JSON, hasil JSON di-deserialize ke tipe `T`.
#[allow(dead_code)]
pub async fn patch_json<T: DeserializeOwned>(
    url: &str,
    headers: Headers,
    body: &(impl Serialize + ?Sized),
) -> Result<T, AppError> {
    send::<T>(Method::PATCH, url, headers, Some(body), DEFAULT_TIMEOUT).await
}

/// DELETE, hasil JSON di-deserialize ke tipe `T`.
#[allow(dead_code)]
pub async fn delete_json<T: DeserializeOwned>(url: &str, headers: Headers) -> Result<T, AppError> {
    send::<T>(Method::DELETE, url, headers, Option::<&()>::None, DEFAULT_TIMEOUT).await
}
