//! Klien Binance — SATU-SATUNYA tempat yang berkomunikasi dengan exchange (REST, WebSocket,
//! request bertanda tangan). Service lain cukup memanggil fungsi di sini, tidak menyusun URL
//! atau signature sendiri. Base URL dari `.env` (`MARKET_API_BASE_URL`/`MARKET_WS_BASE_URL`), jadi
//! pindah ke exchange kompatibel (mis. Tokocrypto) cukup ganti env.
//!
//! Request yang memakai API key user (saldo, order) bisa diarahkan ke exchange lain lewat
//! `ACCOUNT_API_BASE_URL` — mis. Spot Testnet (`https://testnet.binance.vision`) untuk tes order
//! tanpa uang asli — sementara data pasar (candle, harga, WebSocket) tetap dari exchange asli.

use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hmac::{Hmac, Mac};
use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::Value;
use sha2::Sha256;

use crate::utils::app_error::AppError;
use crate::utils::http_client::{
    get_json, get_json_with_headers, get_json_with_headers_error_body, get_json_with_timeout,
    post_with_headers_error_body, ErrorResponse,
};

fn api_base_url() -> String {
    std::env::var("MARKET_API_BASE_URL").unwrap_or_else(|_| "https://api.binance.com".to_string())
}

fn ws_base_url() -> String {
    std::env::var("MARKET_WS_BASE_URL")
        .unwrap_or_else(|_| "wss://stream.binance.com:9443/ws".to_string())
}

/// Exchange tujuan request ber-API-key (saldo, order). Kosong = sama dengan data pasar.
fn account_api_base_url() -> String {
    std::env::var("ACCOUNT_API_BASE_URL")
        .ok()
        .map(|url| url.trim().trim_end_matches('/').to_string())
        .filter(|url| !url.is_empty())
        .unwrap_or_else(api_base_url)
}

/// `true` kalau request akun diarahkan ke Spot Testnet (uang mainan).
pub fn is_testnet() -> bool {
    account_api_base_url().contains("testnet")
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

/// Respons `exchangeInfo` besar (±6 MB walau sudah difilter TRADING; tanpa filter ±18 MB) — lewat
/// koneksi lambat/VPN butuh puluhan detik, jadi batas waktunya dilonggarkan.
const EXCHANGE_INFO_TIMEOUT: Duration = Duration::from_secs(120);

/// Pair SPOT yang sedang `TRADING` (`GET /api/v3/exchangeInfo?permissions=SPOT&symbolStatus=TRADING`).
/// Pair yang sedang tidak diperdagangkan sengaja tidak diminta supaya respons ±3x lebih kecil.
pub async fn get_spot_symbols() -> Result<Vec<ExchangeSymbol>, AppError> {
    let url = format!(
        "{}/api/v3/exchangeInfo?permissions=SPOT&symbolStatus=TRADING",
        api_base_url()
    );
    let info: ExchangeInfo = get_json_with_timeout(&url, EXCHANGE_INFO_TIMEOUT).await?;
    Ok(info.symbols)
}

/// Aturan order 1 pair dari `exchangeInfo`: kelipatan qty (`LOT_SIZE.stepSize`) & nilai order
/// minimum dalam USDT (`NOTIONAL`/`MIN_NOTIONAL`).
#[derive(Debug, Clone, Copy)]
pub struct TradingRules {
    pub step_size: Decimal,
    pub min_notional: Decimal,
}

/// Aturan order beberapa pair sekaligus (`GET /api/v3/exchangeInfo?symbols=[...]`, respons kecil).
pub async fn get_trading_rules(symbols: &[String]) -> Result<HashMap<String, TradingRules>, AppError> {
    trading_rules_at(&api_base_url(), symbols).await
}

/// Seperti `get_trading_rules`, dari exchange tempat order user dikirim (`ACCOUNT_API_BASE_URL`).
pub async fn get_account_trading_rules(
    symbols: &[String],
) -> Result<HashMap<String, TradingRules>, AppError> {
    trading_rules_at(&account_api_base_url(), symbols).await
}

async fn trading_rules_at(
    base_url: &str,
    symbols: &[String],
) -> Result<HashMap<String, TradingRules>, AppError> {
    if symbols.is_empty() {
        return Ok(HashMap::new());
    }
    let list = serde_json::to_string(symbols)
        .map_err(|_| AppError::Internal("Gagal menyusun daftar simbol".to_string()))?;
    let url = format!("{base_url}/api/v3/exchangeInfo?symbols={list}");
    let info: Value = get_json(&url).await?;

    let decimal = |filter: &Value, key: &str| -> Option<Decimal> {
        filter.get(key)?.as_str()?.parse().ok()
    };
    let mut rules = HashMap::new();
    for symbol in info["symbols"].as_array().into_iter().flatten() {
        let Some(name) = symbol["symbol"].as_str() else { continue };
        let filters = symbol["filters"].as_array().cloned().unwrap_or_default();
        let find = |kind: &str| filters.iter().find(|f| f["filterType"] == kind).cloned();

        let step_size = find("LOT_SIZE").and_then(|f| decimal(&f, "stepSize"));
        let min_notional = find("NOTIONAL")
            .and_then(|f| decimal(&f, "minNotional"))
            .or_else(|| find("MIN_NOTIONAL").and_then(|f| decimal(&f, "minNotional")));
        if let (Some(step_size), Some(min_notional)) = (step_size, min_notional) {
            rules.insert(name.to_string(), TradingRules { step_size, min_notional });
        }
    }
    Ok(rules)
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

#[derive(Debug, Deserialize)]
struct TickerPrice {
    symbol: String,
    price: Decimal,
}

/// Harga terakhir 1 pair (`GET /api/v3/ticker/price?symbol=`).
pub async fn get_price(symbol: &str) -> Result<Decimal, AppError> {
    let url = format!("{}/api/v3/ticker/price?symbol={symbol}", api_base_url());
    let ticker: TickerPrice = get_json(&url).await?;
    Ok(ticker.price)
}

/// Harga terakhir SEMUA pair dalam 1 request (bobot rate limit kecil), dipetakan per simbol.
pub async fn get_all_prices() -> Result<HashMap<String, Decimal>, AppError> {
    prices_at(&api_base_url()).await
}

/// Seperti `get_all_prices`, dari exchange tempat saldo & order user berada
/// (`ACCOUNT_API_BASE_URL`) — dipakai untuk menilai saldo akun LIVE.
pub async fn get_account_prices() -> Result<HashMap<String, Decimal>, AppError> {
    prices_at(&account_api_base_url()).await
}

async fn prices_at(base_url: &str) -> Result<HashMap<String, Decimal>, AppError> {
    let url = format!("{base_url}/api/v3/ticker/price");
    let tickers: Vec<TickerPrice> = get_json(&url).await?;
    Ok(tickers.into_iter().map(|t| (t.symbol, t.price)).collect())
}

/// Ticker 24 jam SEMUA pair dalam 1 request (tanpa parameter `symbol`). Bobot rate limit-nya
/// besar (80) — panggil seperlunya, jangan per request user tanpa cache.
pub async fn get_tickers_24hr() -> Result<Vec<Ticker24hr>, AppError> {
    let url = format!("{}/api/v3/ticker/24hr", api_base_url());
    get_json(&url).await
}

/// Ticker 24 jam beberapa pair saja (`GET /api/v3/ticker/24hr?symbols=[...]`) — respons kecil,
/// bobot 2 untuk 1–20 simbol, 40 untuk 21–100. Satu simbol tidak dikenal exchange = seluruh
/// request ditolak (400), jadi pemanggil sebaiknya punya fallback.
pub async fn get_tickers_24hr_for(symbols: &[String]) -> Result<Vec<Ticker24hr>, AppError> {
    if symbols.is_empty() {
        return Ok(Vec::new());
    }
    let list = serde_json::to_string(symbols)
        .map_err(|_| AppError::Internal("Gagal menyusun daftar simbol".to_string()))?;
    let url = format!("{}/api/v3/ticker/24hr?symbols={list}", api_base_url());
    get_json(&url).await
}

// ---------------------------------------------------------------------------------------------
// WebSocket stream
// ---------------------------------------------------------------------------------------------

/// URL stream candle live 1 simbol+interval.
pub fn kline_stream_url(symbol: &str, interval: &str) -> String {
    format!("{}/{}@kline_{interval}", ws_base_url(), symbol.to_lowercase())
}

/// URL stream tanpa stream awal — stream dipilih belakangan lewat pesan `SUBSCRIBE`
/// (`stream_command`), jadi daftar simbol bisa berubah tanpa memutus koneksi.
pub fn stream_base_url() -> String {
    ws_base_url()
}

/// Nama stream mini ticker 24 jam 1 simbol (update tiap 1 detik kalau harganya berubah).
pub fn mini_ticker_stream(symbol: &str) -> String {
    format!("{}@miniTicker", symbol.to_lowercase())
}

/// Pesan `SUBSCRIBE`/`UNSUBSCRIBE` untuk koneksi stream yang sudah terbuka.
pub fn stream_command(method: &str, streams: &[String], id: u64) -> String {
    serde_json::json!({ "method": method, "params": streams, "id": id }).to_string()
}

/// Payload event stream `<symbol>@miniTicker`. `open` = harga 24 jam lalu (rolling), jadi
/// persen naik/turun-nya sama dengan `priceChangePercent` di REST `/ticker/24hr`.
#[derive(Debug, Deserialize)]
pub struct MiniTickerEvent {
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "c")]
    pub close: String,
    #[serde(rename = "o")]
    pub open: String,
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
    #[serde(rename = "canTrade")]
    can_trade: bool,
    #[serde(default)]
    balances: Vec<AssetBalance>,
}

/// Saldo 1 aset di akun spot: `free` = bisa dipakai, `locked` = tertahan di order terbuka.
#[derive(Debug, Clone, Deserialize)]
pub struct AssetBalance {
    pub asset: String,
    pub free: Decimal,
    pub locked: Decimal,
}

/// Saldo semua aset akun spot user yang tidak nol (`GET /api/v3/account`, bertanda tangan).
pub async fn get_account_balances(api_key: &str, api_secret: &str) -> Result<Vec<AssetBalance>, AppError> {
    let url = format!(
        "{}/api/v3/account?{}",
        account_api_base_url(),
        signed_query("omitZeroBalances=true", api_secret)?
    );
    let account: AccountInfo =
        get_json_with_headers(&url, vec![("X-MBX-APIKEY", api_key.to_string())])
            .await
            .map_err(|_| {
                AppError::BadRequest(
                    "Gagal membaca saldo Binance — cek API key/secret di profil".to_string(),
                )
            })?;
    Ok(account.balances)
}

/// Body error Binance, mis. `{"code":-2015,"msg":"Invalid API-key, IP, or permissions for action."}`.
#[derive(Debug, Deserialize)]
struct BinanceError {
    code: i64,
    #[serde(default)]
    msg: String,
}

/// Izin 1 API key (`GET /sapi/v1/account/apiRestrictions`).
#[derive(Debug, Deserialize)]
struct ApiRestrictions {
    #[serde(rename = "enableSpotAndMarginTrading", default)]
    enable_spot_trading: bool,
    #[serde(rename = "enableWithdrawals", default)]
    enable_withdrawals: bool,
}

/// Terjemahkan penolakan Binance jadi pesan yang bisa ditindaklanjuti user.
fn credential_error(err: ErrorResponse) -> AppError {
    tracing::warn!("Verifikasi kredensial Binance ditolak (status {}): {}", err.status, err.body);
    let code = serde_json::from_str::<BinanceError>(&err.body).ok().map(|e| e.code);
    let message = match code {
        Some(-2014) => "Format API key tidak valid — salin ulang API key dari Binance",
        Some(-2015) => {
            "API key ditolak Binance: key salah/sudah dihapus, IP server belum diizinkan, \
             atau izin key kurang"
        }
        Some(-1022) => "API secret salah — tidak cocok dengan API key-nya",
        Some(-1021) => {
            return AppError::Internal("Jam server tidak sinkron dengan Binance, coba lagi".to_string());
        }
        _ => "API key/secret tidak valid atau tidak punya izin akses ke Binance",
    };
    AppError::BadRequest(message.to_string())
}

/// Pastikan `api_key`/`api_secret` siap dipakai robot sebelum disimpan:
/// 1. key & secret valid dan akunnya boleh trading (`GET /api/v3/account`);
/// 2. izin key: Spot Trading wajib aktif, Withdrawals wajib mati — supaya kalau key bocor,
///    dana tidak bisa ditarik (`GET /sapi/v1/account/apiRestrictions`). Spot Testnet tidak punya
///    endpoint `/sapi`, jadi langkah ini dilewati di testnet (key testnet memang hanya bisa trading).
pub async fn verify_credentials(api_key: &str, api_secret: &str) -> Result<(), AppError> {
    let headers = || vec![("X-MBX-APIKEY", api_key.to_string())];
    let base_url = account_api_base_url();

    let url = format!(
        "{base_url}/api/v3/account?{}",
        signed_query("omitZeroBalances=true", api_secret)?
    );
    let account: AccountInfo = get_json_with_headers_error_body(&url, headers())
        .await?
        .map_err(credential_error)?;
    if !account.can_trade {
        return Err(AppError::BadRequest(
            "Akun Binance ini sedang tidak bisa trading — cek status akun di Binance".to_string(),
        ));
    }
    if is_testnet() {
        return Ok(());
    }

    let url = format!(
        "{base_url}/sapi/v1/account/apiRestrictions?{}",
        signed_query("", api_secret)?
    );
    let restrictions: ApiRestrictions = get_json_with_headers_error_body(&url, headers())
        .await?
        .map_err(credential_error)?;
    if !restrictions.enable_spot_trading {
        return Err(AppError::BadRequest(
            "Aktifkan izin \"Enable Spot & Margin Trading\" pada API key ini di Binance".to_string(),
        ));
    }
    if restrictions.enable_withdrawals {
        return Err(AppError::BadRequest(
            "Demi keamanan, matikan izin \"Enable Withdrawals\" pada API key ini di Binance"
                .to_string(),
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Order (uang asli kalau ACCOUNT_API_BASE_URL bukan testnet)
// ---------------------------------------------------------------------------------------------

/// Besar order market: `Quote` = belanja USDT sebanyak ini (`quoteOrderQty`, untuk BUY),
/// `Base` = jual coin sebanyak ini (`quantity`, untuk SELL, sudah dibulatkan ke `stepSize`).
#[derive(Debug, Clone, Copy)]
pub enum OrderAmount {
    Quote(Decimal),
    Base(Decimal),
}

/// Fee 1 eksekusi (fill) dari order market. Harga & qty per fill tidak perlu dibaca — totalnya
/// sudah ada di `executedQty`/`cummulativeQuoteQty`, dan respons lengkap tetap disimpan apa adanya.
#[derive(Debug, Clone, Deserialize)]
pub struct OrderFill {
    pub commission: Decimal,
    #[serde(rename = "commissionAsset")]
    pub commission_asset: String,
}

/// Bagian respons `POST /api/v3/order` (`newOrderRespType=FULL`) yang dipakai untuk mencatat posisi.
#[derive(Debug, Clone, Deserialize)]
pub struct OrderResult {
    #[serde(rename = "orderId")]
    pub order_id: i64,
    pub status: String,
    /// Jumlah coin yang benar-benar tereksekusi (sebelum fee).
    #[serde(rename = "executedQty")]
    pub executed_qty: Decimal,
    /// Total USDT yang berpindah (sebelum fee).
    #[serde(rename = "cummulativeQuoteQty")]
    pub quote_qty: Decimal,
    #[serde(default)]
    pub fills: Vec<OrderFill>,
}

/// Penolakan order (saldo kurang, di bawah minimum, dsb) — pesan asli Binance dipertahankan
/// karena ini untuk log/audit robot, bukan ditampilkan mentah ke user.
fn order_error(symbol: &str, err: ErrorResponse) -> AppError {
    match serde_json::from_str::<BinanceError>(&err.body) {
        Ok(e) => AppError::BadRequest(format!("Order {symbol} ditolak Binance ({}): {}", e.code, e.msg)),
        Err(_) => AppError::BadRequest(format!(
            "Order {symbol} ditolak Binance (status {}): {}",
            err.status, err.body
        )),
    }
}

/// Kirim order MARKET ke exchange akun (`ACCOUNT_API_BASE_URL`). Balik hasil yang sudah di-parse
/// dan respons asli apa adanya (disimpan sebagai bukti order di `trade_positions`).
/// Order yang tidak tereksekusi sama sekali (`executedQty` 0, mis. `EXPIRED`) dianggap gagal.
pub async fn place_market_order(
    api_key: &str,
    api_secret: &str,
    symbol: &str,
    side: &str,
    amount: OrderAmount,
) -> Result<(OrderResult, Value), AppError> {
    let size = match amount {
        OrderAmount::Quote(usdt) => format!("quoteOrderQty={}", usdt.normalize()),
        OrderAmount::Base(qty) => format!("quantity={}", qty.normalize()),
    };
    let params = format!("symbol={symbol}&side={side}&type=MARKET&{size}&newOrderRespType=FULL");
    let url = format!("{}/api/v3/order?{}", account_api_base_url(), signed_query(&params, api_secret)?);

    let raw: Value = post_with_headers_error_body(&url, vec![("X-MBX-APIKEY", api_key.to_string())])
        .await?
        .map_err(|err| order_error(symbol, err))?;
    let result: OrderResult = serde_json::from_value(raw.clone()).map_err(|err| {
        tracing::error!("Respons order {symbol} tidak dikenali: {err} — {raw}");
        AppError::Internal(format!("Respons order {symbol} dari Binance tidak dikenali"))
    })?;
    if result.executed_qty.is_zero() {
        return Err(AppError::BadRequest(format!(
            "Order {symbol} tidak tereksekusi (status {})",
            result.status
        )));
    }
    Ok((result, raw))
}
