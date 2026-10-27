use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::model::StoreListing;
use crate::services::strategies::types::StrategyStats;
use crate::services::strategy_versions::types::BotConfig;

/// Body `POST /api/store/listings` — pasang strategi di Store.
#[derive(Debug, Deserialize)]
pub struct CreateListingInput {
    pub strategy_id: Uuid,
    /// Kosong = nama strategi.
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// `FREE` | `ONE_TIME` | `MONTHLY` (default `FREE`).
    #[serde(default)]
    pub pricing_model: Option<String>,
    /// Harga dalam Rupiah (wajib > 0 untuk ONE_TIME/MONTHLY). Pembayaran saat ini masih dummy (Rp0).
    #[serde(default)]
    pub price_idr: Option<Decimal>,
    /// Lama trial gratis, 0–90 hari (default 0 = tanpa trial).
    #[serde(default)]
    pub trial_days: Option<i16>,
}

/// Body `PUT /api/store/listings/:id`.
#[derive(Debug, Deserialize)]
pub struct UpdateListingInput {
    pub title: Option<String>,
    pub description: Option<String>,
    pub pricing_model: Option<String>,
    pub price_idr: Option<Decimal>,
    pub trial_days: Option<i16>,
    /// `ACTIVE` (tampil) | `INACTIVE` (disembunyikan, pembeli lama tetap bisa memakai).
    pub status: Option<String>,
}

/// Query `GET /api/store/listings`.
#[derive(Debug, Default, Deserialize)]
pub struct BrowseQuery {
    pub page: Option<i64>,
    pub limit: Option<i64>,
    /// Cari judul listing, nama strategi, atau nama pembuat.
    pub search: Option<String>,
    /// `SCREENER` | `BOT`
    pub kind: Option<String>,
    /// `FREE` | `ONE_TIME` | `MONTHLY`
    pub pricing_model: Option<String>,
}

/// Pembuat strategi — ditampilkan dengan nama aslinya.
#[derive(Debug, Serialize)]
pub struct Seller {
    pub user_id: Uuid,
    pub full_name: String,
    pub photo_url: Option<String>,
}

/// Info strategi untuk pembeli (tanpa isi script).
#[derive(Debug, Serialize)]
pub struct ListedStrategy {
    pub id: Uuid,
    pub name: String,
    pub kind: String,
    pub version_no: Option<i32>,
    pub timeframe: &'static str,
    /// Aturan robot & cakupan coin versi aktif.
    pub bot: Option<BotConfig>,
}

/// Hak pakai user yang sedang melihat.
#[derive(Debug, Serialize)]
pub struct MyAccess {
    /// `OWNER` | `PURCHASE` | `TRIAL`
    pub kind: String,
    pub expires_at: Option<DateTime<Utc>>,
}

/// 1 listing di Store. `stats.equity_curve` hanya di detail.
#[derive(Debug, Serialize)]
pub struct ListingCard {
    #[serde(flatten)]
    pub listing: StoreListing,
    pub seller: Option<Seller>,
    pub strategy: ListedStrategy,
    /// Backtest resmi + performa transaksi DEMO & LIVE — sama dengan detail strategi.
    pub stats: StrategyStats,
    /// Pemakai dengan hak pakai yang masih berlaku.
    pub active_users: i64,
    pub my_access: Option<MyAccess>,
    /// User boleh mencoba trial (listing punya trial & user belum pernah trial).
    pub trial_available: bool,
}
