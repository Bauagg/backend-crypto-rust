use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Body `POST /api/transactions` — hanya yang memang keputusan user. Sisanya diisi sistem:
/// user & email dari token, simbol dari `flex_params`, harga masuk & kurs USDT/IDR dari Binance,
/// modal akun dari `demo_balance` (DEMO) atau saldo Binance user (LIVE), waktu buka = sekarang,
/// status `OPEN`.
#[derive(Debug, Deserialize)]
pub struct CreateTradePositionInput {
    /// Coin yang dibeli (`flex_params` SIMBOL_CRYPTO).
    pub flex_param_id: Uuid,
    /// Nilai yang dipakai untuk posisi ini, dalam `currency` (> 0). Kalau `IDR`, sistem
    /// mengonversinya ke USDT pakai kurs Binance saat itu; hasil USDT tidak boleh melebihi modal akun.
    pub entry_value: Decimal,
    /// Mata uang `entry_value`: `IDR` | `USDT`. Opsional — kosong = `preferred_currency` user.
    pub currency: Option<String>,
    /// `DEMO` | `LIVE`
    pub account_mode: String,
}

/// Query param list posisi: search simbol + filter & sort dinamis (format sama dengan flex_params)
/// + pagination. Contoh:
/// `?search=btc&filter=[{"key":"status","operator":"equal","value":"OPEN"}]&sort=opened_at&order=desc&page=1&limit=20`
#[derive(Debug, Default, Deserialize)]
pub struct TradePositionListQuery {
    /// Cari simbol, sebagian & tidak peduli huruf besar/kecil (`btc` cocok dengan `BTCUSDT`).
    pub search: Option<String>,
    /// JSON array `[{"key","operator","value"}]` — operator: equal, notEqual, like, in, gt, gte, lt, lte.
    pub filter: Option<String>,
    /// Nama kolom untuk pengurutan (default `opened_at` terbaru dulu).
    pub sort: Option<String>,
    /// `asc` (default) | `desc`.
    pub order: Option<String>,
    pub page: Option<i64>,
    pub limit: Option<i64>,
}

/// Query param dashboard. Contoh: `?account_mode=LIVE&source=BOT&date_from=2026-06-01&date_to=2026-09-30`
#[derive(Debug, Default, Deserialize)]
pub struct DashboardQuery {
    /// `DEMO` (default) | `LIVE`
    pub account_mode: Option<String>,
    /// `BOT` | `MANUAL` — kosong = keduanya.
    pub source: Option<String>,
    /// Rentang tanggal tutup posisi (UTC, `YYYY-MM-DD`, inklusif) untuk statistik untung/rugi.
    /// Posisi yang masih OPEN selalu ikut (kondisi saat ini).
    pub date_from: Option<NaiveDate>,
    pub date_to: Option<NaiveDate>,
}

/// Data posisi baru — SEMUA kolom wajib diisi pemanggil, tidak ada yang mengandalkan default DB.
/// Kolom penutupan (`closed_at`, `exit_*`, `result`) bertipe `Option` karena untuk posisi `OPEN` nilainya memang harus
/// kosong (dijaga constraint `trade_positions_status_consistent`), tapi tetap wajib ditentukan.
#[derive(Debug, Clone)]
pub struct NewTradePosition {
    pub user_id: Uuid,
    pub user_email: String,
    pub flex_param_id: Uuid,
    pub symbol: String,
    /// `BOT` | `MANUAL`
    pub source: String,
    /// Wajib untuk `BOT`, `None` untuk `MANUAL`.
    pub strategy: Option<String>,
    pub opened_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
    pub holding_days: i32,
    /// `IDR` | `USDT`
    pub currency: String,
    pub usdt_idr_rate: Decimal,
    pub quantity: Decimal,
    pub entry_value: Decimal,
    /// `None` untuk posisi `OPEN`.
    pub exit_value: Option<Decimal>,
    pub entry_price: Decimal,
    /// `None` untuk posisi `OPEN`.
    pub exit_price: Option<Decimal>,
    pub fee_amount: Decimal,
    /// `None` untuk DEMO / posisi manual.
    pub entry_order_id: Option<String>,
    /// `None` untuk DEMO / posisi `OPEN`.
    pub exit_order_id: Option<String>,
    /// Respons asli Binance saat beli (bukti order, akun LIVE). `None` untuk DEMO / manual.
    pub entry_order_response: Option<serde_json::Value>,
    /// Respons asli Binance saat jual. `None` untuk DEMO / posisi `OPEN`.
    pub exit_order_response: Option<serde_json::Value>,
    pub pnl_amount: Decimal,
    pub pnl_percent: Decimal,
    /// `OPEN` | `CLOSED`
    pub status: String,
    /// `PROFIT` | `LOSS` — `None` untuk posisi `OPEN`.
    pub result: Option<String>,
    pub account_capital: Decimal,
    /// `LIVE` | `DEMO`
    pub account_mode: String,
    pub created_by: String,
}

// ---------------------------------------------------------------------------------------------
// Dashboard — semua nilai uang dalam USDT; `usdt_idr_rate` disertakan untuk tampilan Rupiah.
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct Dashboard {
    pub filter: DashboardFilter,
    /// Kurs 1 USDT dalam Rupiah saat ini (`None` kalau kurs gagal diambil).
    pub usdt_idr_rate: Option<Decimal>,
    pub preferred_currency: String,
    pub summary: DashboardSummary,
    /// Nilai modal setelah tiap hari ada posisi yang ditutup (modal awal + untung/rugi terealisasi).
    pub equity: Vec<EquityPoint>,
    pub monthly: Vec<MonthlyPnl>,
    pub per_asset: Vec<AssetStats>,
    /// Isi portofolio sekarang: coin yang masih dipegang + USDT bebas.
    pub allocation: Vec<AllocationItem>,
}

#[derive(Debug, Serialize)]
pub struct DashboardFilter {
    pub account_mode: String,
    pub source: Option<String>,
    pub date_from: Option<NaiveDate>,
    pub date_to: Option<NaiveDate>,
}

#[derive(Debug, Default, Serialize)]
pub struct DashboardSummary {
    /// Modal saat robot mulai (atau modal akun saat posisi pertama dibuka).
    pub initial_capital: Option<Decimal>,
    /// USDT bebas: `demo_balance` (DEMO) / saldo USDT Binance (LIVE). `None` kalau gagal dibaca.
    pub cash: Option<Decimal>,
    /// Nilai coin yang masih dipegang dengan harga sekarang.
    pub open_value: Decimal,
    /// `cash + open_value`.
    pub total_value: Option<Decimal>,
    pub total_return_amount: Option<Decimal>,
    pub total_return_percent: Option<Decimal>,
    /// Untung/rugi posisi yang sudah ditutup (dalam rentang tanggal filter).
    pub realized_pnl: Decimal,
    /// Untung/rugi berjalan posisi OPEN (harga sekarang).
    pub unrealized_pnl: Decimal,
    pub total_fee: Decimal,
    pub closed_trades: usize,
    pub open_positions: usize,
    pub win_count: usize,
    pub loss_count: usize,
    /// Persen trade untung dari trade yang ditutup.
    pub win_rate: Decimal,
    pub avg_profit: Decimal,
    pub avg_loss: Decimal,
    pub best_trade: Option<TradeHighlight>,
    pub worst_trade: Option<TradeHighlight>,
    pub avg_holding_days: Decimal,
    /// Penurunan terdalam kurva modal dari puncaknya (persen, negatif).
    pub max_drawdown_percent: Decimal,
    pub profitable_months: usize,
    pub total_months: usize,
    pub best_month: Option<MonthlyPnl>,
    pub worst_month: Option<MonthlyPnl>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TradeHighlight {
    pub id: Uuid,
    pub symbol: String,
    pub pnl_amount: Decimal,
    pub pnl_percent: Decimal,
    pub closed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EquityPoint {
    pub date: NaiveDate,
    /// Untung/rugi posisi yang ditutup hari itu.
    pub realized_pnl: Decimal,
    pub cumulative_pnl: Decimal,
    /// Modal awal + untung/rugi kumulatif (`None` kalau modal awal tidak diketahui).
    pub equity: Option<Decimal>,
    pub drawdown_percent: Decimal,
}

#[derive(Debug, Clone, Serialize)]
pub struct MonthlyPnl {
    /// `YYYY-MM` (UTC, berdasarkan tanggal tutup posisi).
    pub month: String,
    pub pnl: Decimal,
    /// Untung/rugi bulan itu dibanding modal di awal bulan.
    pub pnl_percent: Option<Decimal>,
    pub trades: usize,
    pub wins: usize,
}

#[derive(Debug, Serialize)]
pub struct AssetStats {
    pub symbol: String,
    pub photo_url: Option<String>,
    pub closed_trades: usize,
    pub wins: usize,
    pub win_rate: Decimal,
    pub realized_pnl: Decimal,
    /// Porsi aset ini dari total untung/rugi terealisasi (persen).
    pub contribution_percent: Option<Decimal>,
    pub best_trade: Option<Decimal>,
    pub worst_trade: Option<Decimal>,
    pub avg_holding_days: Decimal,
    pub open_quantity: Decimal,
    pub open_value: Decimal,
    pub unrealized_pnl: Decimal,
}

#[derive(Debug, Serialize)]
pub struct AllocationItem {
    /// Simbol coin, atau `USDT` untuk saldo bebas.
    pub symbol: String,
    pub photo_url: Option<String>,
    pub value: Decimal,
    pub percent: Decimal,
}
