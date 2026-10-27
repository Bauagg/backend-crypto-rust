use chrono::{DateTime, NaiveDate, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Performa asli posisi strategi per mode akun (DEMO/LIVE dipisah — jangan dicampur).
#[derive(Debug, Clone, Serialize)]
pub struct LivePerformance {
    pub account_mode: String,
    pub closed_trades: i64,
    pub win_rate_percent: Decimal,
    /// Untung/rugi yang sudah terealisasi (USDT).
    pub realized_pnl: Decimal,
    /// `realized_pnl` dibanding modal posisi yang sudah ditutup.
    pub realized_pnl_percent: Decimal,
    pub open_positions: i64,
    pub open_entry_value: Decimal,
    /// Jumlah user pemakai (untuk statistik Store).
    pub users: i64,
    pub first_opened_at: Option<DateTime<Utc>>,
}

impl From<super::repository::StrategyPerformanceRow> for LivePerformance {
    fn from(row: super::repository::StrategyPerformanceRow) -> Self {
        let percent = |part: Decimal, whole: Decimal| {
            if whole.is_zero() { Decimal::ZERO } else { (part / whole * Decimal::ONE_HUNDRED).round_dp(2) }
        };
        Self {
            win_rate_percent: percent(Decimal::from(row.wins), Decimal::from(row.closed_trades)),
            realized_pnl_percent: percent(row.realized_pnl, row.closed_entry_value),
            account_mode: row.account_mode,
            closed_trades: row.closed_trades,
            realized_pnl: row.realized_pnl.round_dp(8),
            open_positions: row.open_positions,
            open_entry_value: row.open_entry_value.round_dp(8),
            users: row.users,
            first_opened_at: row.first_opened_at,
        }
    }
}

/// Body `POST /api/transactions` — BUY & SELL manual, body sama. Hanya yang memang keputusan user;
/// sisanya diisi sistem: user & email dari token, simbol dari `flex_params`, harga & kurs USDT/IDR
/// dari Binance, modal akun dari `demo_balance` (DEMO) atau saldo Binance user (LIVE).
/// - BUY: buka posisi baru senilai `amount` (maksimal modal akun).
/// - SELL: jual coin senilai `amount` — posisi dipilih sistem FIFO (terlama dulu, manual maupun
///   robot), maksimal sebanyak yang dimiliki. `coin_all: true` = jual semua coin itu (tanpa `amount`).
#[derive(Debug, Deserialize)]
pub struct TradeInput {
    /// `BUY` | `SELL`
    pub transaction_type: String,
    /// Coin yang dibeli/dijual (`flex_params` SIMBOL_CRYPTO).
    pub flex_param_id: Uuid,
    /// `DEMO` | `LIVE`
    pub account_mode: String,
    /// Nilai yang dibeli/dijual dalam `currency` (> 0), mis. "30" = senilai $30. Kalau `IDR`,
    /// dikonversi ke USDT pakai kurs Binance saat itu. Wajib, kecuali SELL dengan `coin_all: true`.
    #[serde(default)]
    pub amount: Option<Decimal>,
    /// Khusus SELL: `true` = jual SEMUA coin ini yang dimiliki, berapa pun harganya saat order
    /// (`amount` diabaikan) — tidak tergantung harga yang sudah bergerak sejak layar dibuka.
    #[serde(default)]
    pub coin_all: bool,
    /// Mata uang `amount`: `IDR` | `USDT`. Opsional — kosong = `preferred_currency` user.
    #[serde(default)]
    pub currency: Option<String>,
    /// Asal keputusan BUY, dicatat di `trade_positions.rule_type` sebagai bahan analisa strategi
    /// yang profit: `MANUAL` (default) | `SCREENER` (dari hasil screening, wajib `screener_id`) |
    /// `RECOMMENDATION` (dari rekomendasi coin Python) | `STRATEGY` (dari hasil strategi script,
    /// wajib `strategy_subscription_id`). Kosong = ditebak dari id yang diisi.
    /// Diabaikan untuk SELL.
    #[serde(default)]
    pub rule_type: Option<String>,
    /// Id strategi screener user (`GET /screeners` -> `id`) untuk `rule_type = SCREENER`.
    #[serde(default)]
    pub screener_id: Option<Uuid>,
    /// Id pemakaian strategi script (`GET /strategy-subscriptions` -> `id`) untuk
    /// `rule_type = STRATEGY` — posisi dicatat dengan `rule_id` = versi strategi yang dipakai.
    #[serde(default)]
    pub strategy_subscription_id: Option<Uuid>,
}

/// Query param list posisi: search simbol, filter & sort dinamis (format sama dengan flex_params),
/// dan pagination. Contoh:
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
    /// `ROBOT` | `MANUAL` | `SCREENER` — lihat migration 0019.
    pub rule_type: String,
    /// `user_screeners.id` untuk `SCREENER`; `None` untuk `ROBOT` & `MANUAL`.
    pub rule_id: Option<Uuid>,
    /// Bot strategi yang membuka posisi (`strategy_subscriptions.id`); `None` selain itu.
    pub strategy_subscription_id: Option<Uuid>,
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


/// Hasil jual manual.
#[derive(Debug, Serialize)]
pub struct ManualSellResult {
    pub symbol: String,
    pub account_mode: String,
    pub sold_quantity: Decimal,
    /// Harga jual rata-rata.
    pub exit_price: Decimal,
    /// USDT bersih diterima (sudah dipotong fee jual).
    pub exit_value: Decimal,
    pub fee_amount: Decimal,
    pub pnl_amount: Decimal,
    pub pnl_percent: Decimal,
    /// `PROFIT` | `LOSS`
    pub result: String,
    /// Jumlah posisi yang ikut terjual (FIFO).
    pub positions_sold: usize,
    /// Sisa coin ini yang masih dipegang (0 = habis).
    pub remaining_quantity: Decimal,
    /// orderId Binance (LIVE); `None` untuk DEMO.
    pub order_id: Option<String>,
}

/// Query `GET /api/transactions/holding?flex_param_id=&account_mode=`.
#[derive(Debug, Deserialize)]
pub struct HoldingQuery {
    pub flex_param_id: Uuid,
    /// `DEMO` (default) | `LIVE`
    #[serde(default)]
    pub account_mode: Option<String>,
}

/// Total 1 coin yang dimiliki user (posisi OPEN manual + robot) — untuk layar Sell ("Dimiliki: …").
#[derive(Debug, Serialize)]
pub struct Holding {
    pub flex_param_id: Uuid,
    pub symbol: String,
    pub photo_url: Option<String>,
    pub account_mode: String,
    /// Total coin yang dimiliki = batas maksimal yang bisa dijual.
    pub quantity: Decimal,
    /// Bagian dari `quantity` hasil beli manual.
    pub manual_quantity: Decimal,
    /// Bagian dari `quantity` hasil robot.
    pub bot_quantity: Decimal,
    /// Total USDT yang dulu dipakai membeli.
    pub entry_value: Decimal,
    pub avg_entry_price: Decimal,
    pub current_price: Decimal,
    /// Nilai sekarang (`quantity × current_price`) — maksimal `amount` yang bisa dijual.
    pub current_value: Decimal,
    pub unrealized_pnl: Decimal,
    pub unrealized_pnl_percent: Decimal,
    /// Jumlah posisi OPEN yang membentuk total ini.
    pub open_positions: usize,
    /// Saldo USDT yang tidak sedang dipakai di transaksi — maksimal `amount` untuk BUY.
    /// DEMO: `demo_balance`; LIVE: USDT bebas di Binance (`null` kalau API key belum diisi).
    pub available_balance: Option<Decimal>,
    /// Rincian posisi OPEN, terlama dulu (urutan yang dijual duluan saat SELL).
    pub positions: Vec<HoldingPosition>,
}

/// 1 posisi OPEN di dalam `Holding`, dinilai dengan harga sekarang.
#[derive(Debug, Serialize)]
pub struct HoldingPosition {
    pub id: Uuid,
    /// `MANUAL` | `BOT`
    pub source: String,
    pub strategy: Option<String>,
    pub opened_at: DateTime<Utc>,
    pub holding_days: i64,
    pub quantity: Decimal,
    pub entry_price: Decimal,
    pub entry_value: Decimal,
    pub current_value: Decimal,
    pub unrealized_pnl: Decimal,
    pub unrealized_pnl_percent: Decimal,
}

/// Query `GET /api/transactions/holding/:symbol?account_mode=`.
#[derive(Debug, Deserialize)]
pub struct HoldingBySymbolQuery {
    /// `DEMO` (default) | `LIVE`
    #[serde(default)]
    pub account_mode: Option<String>,
}
