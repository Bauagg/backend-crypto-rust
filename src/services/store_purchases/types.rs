use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::model::StorePurchase;

/// Body `POST /api/store/purchases/quote` & `POST /api/store/purchases`.
#[derive(Debug, Deserialize)]
pub struct CheckoutInput {
    pub listing_id: Uuid,
    /// Kode promo dari pembuat (opsional, tidak berlaku untuk trial).
    #[serde(default)]
    pub promo_code: Option<String>,
    /// `true` = pakai trial gratis listing (sekali per user per listing).
    #[serde(default)]
    pub trial: bool,
}

/// Rincian harga sebelum/saat checkout.
#[derive(Debug, Serialize)]
pub struct Quote {
    pub listing_id: Uuid,
    /// `PURCHASE` | `TRIAL`
    pub kind: &'static str,
    pub pricing_model: String,
    pub list_price_idr: Decimal,
    pub discount_percent: Decimal,
    pub discount_idr: Decimal,
    /// Harga setelah promo.
    pub final_price_idr: Decimal,
    /// Yang benar-benar dibayar sekarang — Rp0 selama pembayaran masih dummy.
    pub pay_now_idr: Decimal,
    /// Lama hak pakai dalam hari; `None` = selamanya.
    pub duration_days: Option<i64>,
    pub promo_code: Option<String>,
}

/// 1 hak pakai + info listing-nya.
#[derive(Debug, Serialize)]
pub struct PurchaseView {
    #[serde(flatten)]
    pub purchase: StorePurchase,
    pub listing_title: Option<String>,
    /// Hak pakai masih berlaku sekarang.
    pub is_active: bool,
}
