use reqwest::Method;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::utils::app_error::AppError;

/// Header tambahan untuk request ke API pihak ketiga, mis. API key exchange
/// (`X-MBX-APIKEY` untuk Binance, signature, dsb).
pub type Headers = Vec<(&'static str, String)>;

async fn send<T: DeserializeOwned>(
    method: Method,
    url: &str,
    headers: Headers,
    body: Option<&(impl Serialize + ?Sized)>,
) -> Result<T, AppError> {
    let client = reqwest::Client::new();
    let mut request = client.request(method, url);

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

/// GET dengan header tambahan (mis. API key exchange di `X-MBX-APIKEY`).
#[allow(dead_code)]
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
