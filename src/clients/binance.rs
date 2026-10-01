//! Klien Binance — SATU-SATUNYA tempat yang berkomunikasi dengan exchange (REST, WebSocket,
//! request bertanda tangan). Service lain cukup memanggil fungsi di sini, tidak menyusun URL
//! atau signature sendiri. Base URL dari `.env` (`MARKET_API_BASE_URL`/`MARKET_WS_BASE_URL`), jadi
//! pindah ke exchange kompatibel (mis. Tokocrypto) cukup ganti env.

use std::time::{SystemTime, UNIX_EPOCH};

use hmac::{Hmac, Mac};
use serde::Deserialize;
use serde_json::Value;
use sha2::Sha256;

use crate::utils::app_error::AppError;
use crate::utils::http_client::{get_json, get_json_with_headers};

fn api_base_url() -> String {
    std::env::var("MARKET_API_BASE_URL").unwrap_or_else(|_| "https://api.binance.com".to_string())
}

fn ws_base_url() -> String {
    std::env::var("MARKET_WS_BASE_URL")
        .unwrap_or_else(|_| "wss://stream.binance.com:9443/ws".to_string())
}

// ---------------------------------------------------------------------------------------------
// Market data (publik, tanpa API key)
// ---------------------------------------------------------------------------------------------

/// 1 candle dari REST `/api/v3/klines`. Angka tetap string (seperti dari exchange) supaya tidak
/// ada pembulatan — pemanggil yang memutuskan mau di-parse ke `Decimal` atau diteruskan apa adanya.
#[derive(Debug, Clone)]
pub struct Kline {
    pub open_time: i64,
    pub open: String,
    pub high: String,
    pub low: String,
    pub close: String,
    pub volume: String,
    pub close_time: i64,
}

/// `GET /api/v3/klines`. `start_time`/`end_time` opsional (ms, berdasarkan `open_time`, inklusif);
/// tanpa keduanya = `limit` candle terakhir. Maksimal 1000 candle per request.
pub async fn get_klines(
    symbol: &str,
    interval: &str,
    limit: u16,
    start_time: Option<i64>,
    end_time: Option<i64>,
) -> Result<Vec<Kline>, AppError> {
    let mut url = format!(
        "{}/api/v3/klines?symbol={symbol}&interval={interval}&limit={limit}",
        api_base_url()
    );
    if let Some(start_time) = start_time {
        url.push_str(&format!("&startTime={start_time}"));
    }
    if let Some(end_time) = end_time {
        url.push_str(&format!("&endTime={end_time}"));
    }

    let raw: Vec<Value> = get_json(&url).await?;
    Ok(raw.iter().filter_map(|row| parse_kline(row.as_array()?)).collect())
}

/// 1 baris /api/v3/klines: [open_time, open, high, low, close, volume, close_time, ...].
fn parse_kline(row: &[Value]) -> Option<Kline> {
    let text = |i: usize| Some(row.get(i)?.as_str()?.to_string());
    Some(Kline {
        open_time: row.first()?.as_i64()?,
        open: text(1)?,
        high: text(2)?,
        low: text(3)?,
        close: text(4)?,
        volume: text(5)?,
        close_time: row.get(6)?.as_i64()?,
    })
}

#[derive(Debug, Deserialize)]
struct ExchangeInfo {
    symbols: Vec<ExchangeSymbol>,
}

/// 1 pair dari `/api/v3/exchangeInfo`.
#[derive(Debug, Deserialize)]
pub struct ExchangeSymbol {
    pub symbol: String,
    pub status: String,
    #[serde(rename = "baseAsset")]
    pub base_asset: String,
    #[serde(rename = "quoteAsset")]
    pub quote_asset: String,
    #[serde(rename = "isSpotTradingAllowed", default)]
    pub is_spot_trading_allowed: bool,
}

/// Semua pair yang punya izin SPOT (`GET /api/v3/exchangeInfo?permissions=SPOT`), termasuk yang
/// sedang tidak `TRADING` — pemanggil yang menyaring.
pub async fn get_spot_symbols() -> Result<Vec<ExchangeSymbol>, AppError> {
    let url = format!("{}/api/v3/exchangeInfo?permissions=SPOT", api_base_url());
    let info: ExchangeInfo = get_json(&url).await?;
    Ok(info.symbols)
}

/// 1 item `/api/v3/ticker/24hr`.
#[derive(Debug, Clone, Deserialize)]
pub struct Ticker24hr {
    pub symbol: String,
    #[serde(rename = "lastPrice")]
    pub last_price: String,
    #[serde(rename = "priceChangePercent")]
    pub price_change_percent: String,
    /// Volume 24 jam dalam quote asset (mis. USDT), bukan dalam jumlah coin.
    #[serde(rename = "quoteVolume")]
    pub quote_volume: String,
}

/// Ticker 24 jam SEMUA pair dalam 1 request (tanpa parameter `symbol`). Bobot rate limit-nya
/// besar (80) — panggil seperlunya, jangan per request user tanpa cache.
pub async fn get_tickers_24hr() -> Result<Vec<Ticker24hr>, AppError> {
    let url = format!("{}/api/v3/ticker/24hr", api_base_url());
    get_json(&url).await
}

// ---------------------------------------------------------------------------------------------
// WebSocket stream
// ---------------------------------------------------------------------------------------------

/// URL stream candle live 1 simbol+interval.
pub fn kline_stream_url(symbol: &str, interval: &str) -> String {
    format!("{}/{}@kline_{interval}", ws_base_url(), symbol.to_lowercase())
}

/// Payload event stream kline.
#[derive(Debug, Deserialize)]
pub struct KlineEvent {
    pub k: KlineEventData,
}

#[derive(Debug, Deserialize)]
pub struct KlineEventData {
    #[serde(rename = "t")]
    pub open_time: i64,
    #[serde(rename = "T")]
    pub close_time: i64,
    #[serde(rename = "o")]
    pub open: String,
    #[serde(rename = "h")]
    pub high: String,
    #[serde(rename = "l")]
    pub low: String,
    #[serde(rename = "c")]
    pub close: String,
    #[serde(rename = "v")]
    pub volume: String,
    /// `true` = candle ini sudah close (event terakhir untuk candle tersebut).
    #[serde(rename = "x")]
    pub is_closed: bool,
}

// ---------------------------------------------------------------------------------------------
// Akun (request bertanda tangan, butuh API key & secret milik user)
// ---------------------------------------------------------------------------------------------

/// Query string + `signature` HMAC-SHA256 sesuai spesifikasi resmi Binance — `api_secret` hanya
/// dipakai untuk menandatangani, tidak pernah dikirim.
fn signed_query(params: &str, api_secret: &str) -> Result<String, AppError> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| AppError::Internal("Gagal membaca waktu sistem".to_string()))?
        .as_millis();
    let query = if params.is_empty() {
        format!("timestamp={timestamp}")
    } else {
        format!("{params}&timestamp={timestamp}")
    };

    let mut mac = Hmac::<Sha256>::new_from_slice(api_secret.as_bytes())
        .map_err(|_| AppError::Internal("Gagal membuat signature".to_string()))?;
    mac.update(query.as_bytes());
    let signature = hex::encode(mac.finalize().into_bytes());

    Ok(format!("{query}&signature={signature}"))
}

/// Respons minimal `GET /api/v3/account` — field lain belum dibutuhkan.
#[derive(Debug, Deserialize)]
struct AccountInfo {
    #[allow(dead_code)]
    #[serde(rename = "canTrade")]
    can_trade: bool,
}

/// Pastikan `api_key`/`api_secret` valid & aktif dengan memanggil endpoint bertanda tangan
/// `GET /api/v3/account` — kredensial salah/dicabut dibalas 401 oleh Binance.
pub async fn verify_credentials(api_key: &str, api_secret: &str) -> Result<(), AppError> {
    let url = format!("{}/api/v3/account?{}", api_base_url(), signed_query("", api_secret)?);
    get_json_with_headers::<AccountInfo>(&url, vec![("X-MBX-APIKEY", api_key.to_string())])
        .await
        .map(|_| ())
        .map_err(|_| {
            AppError::BadRequest(
                "API key/secret tidak valid atau tidak punya izin akses ke Binance".to_string(),
            )
        })
}
