use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::Serialize;
use uuid::Uuid;

/// 1 posisi trading: 1 siklus beli -> jual satu coin milik user, akun LIVE maupun DEMO.
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct TradePosition {
    pub id: Uuid,
    pub user_id: Uuid,
    /// Snapshot email pemilik posisi.
    pub user_email: String,
    /// Coin yang diperdagangkan (`flex_params` SIMBOL_CRYPTO).
    pub flex_param_id: Uuid,
    /// Snapshot simbol saat posisi dibuka, mis. `BTCUSDT`.
    pub symbol: String,
    /// Siapa yang membuka posisi: `BOT` (robot, dari /signal Python) | `MANUAL` (user).
    pub source: String,
    /// Strategi Python yang menghasilkan posisi (mis. `V23`, `BTC-60`); `None` untuk posisi manual.
    pub strategy: Option<String>,
    pub opened_at: DateTime<Utc>,
    /// `None` selama posisi masih `OPEN`.
    pub closed_at: Option<DateTime<Utc>>,
    pub holding_days: i32,
    /// Mata uang yang dipakai user saat input: `IDR` | `USDT`. Kolom nilai tetap dalam USDT.
    pub currency: String,
    /// Kurs 1 USDT dalam Rupiah (Binance `USDTIDR`) saat posisi dibuka.
    pub usdt_idr_rate: Decimal,
    /// Jumlah coin yang dipegang (qty asli setelah fee) — dikirim ke /signal sebagai `posisi`.
    pub quantity: Decimal,
    /// Nilai USDT yang dipakai saat masuk posisi.
    pub entry_value: Decimal,
    /// USDT yang diterima saat posisi ditutup — `None` selama posisi masih `OPEN`.
    pub exit_value: Option<Decimal>,
    /// Harga aktual simbol saat beli.
    pub entry_price: Decimal,
    /// Harga aktual simbol saat posisi ditutup — `None` selama posisi masih `OPEN`.
    pub exit_price: Option<Decimal>,
    /// Total fee exchange (beli + jual) dalam USDT.
    pub fee_amount: Decimal,
    /// orderId Binance saat beli — `None` untuk akun DEMO / posisi manual.
    pub entry_order_id: Option<String>,
    /// orderId Binance saat jual — `None` untuk DEMO / posisi yang belum ditutup.
    pub exit_order_id: Option<String>,
    /// Bukti order beli berhasil di Binance (akun LIVE): respons asli `POST /api/v3/order`.
    pub entry_order_response: Option<serde_json::Value>,
    /// Bukti order jual berhasil di Binance (akun LIVE): respons asli `POST /api/v3/order`.
    pub exit_order_response: Option<serde_json::Value>,
    /// Untung/rugi dalam USDT (final saat `CLOSED`).
    pub pnl_amount: Decimal,
    pub pnl_percent: Decimal,
    /// `OPEN` | `CLOSED`
    pub status: String,
    /// `PROFIT` | `LOSS` — `None` selama posisi masih `OPEN`.
    pub result: Option<String>,
    /// Total modal akun saat posisi dibuka.
    pub account_capital: Decimal,
    /// `LIVE` (Binance asli) | `DEMO` (simulasi).
    pub account_mode: String,
    pub created_by: String,
    pub updated_by: String,
    pub deleted_by: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deleted_at: Option<DateTime<Utc>>,
}

/// Posisi + logo coin dari `flex_params` — bentuk yang dikirim ke FE (list & detail). JSON-nya
/// datar: semua field posisi + `photo_url`.
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct TradePositionWithPhoto {
    #[sqlx(flatten)]
    #[serde(flatten)]
    pub position: TradePosition,
    /// Logo coin (`flex_params.photo_url`), `None` kalau coin belum punya foto.
    pub photo_url: Option<String>,
}
