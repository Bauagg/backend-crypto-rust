use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;
use validator::Validate;

use super::model::UserScreener;

/// Satu-satunya timeframe data screening: candle harian.
pub const TIMEFRAME: &str = "1D";
/// Keterangan untuk ditampilkan FE di daftar strategi, hasil screening, & chat AI.
pub const TIMEFRAME_NOTE: &str = "Strategi ini dinilai dari data candle harian (OHLC timeframe 1D / D1). \
Timeframe kecil (1 menit, 15 menit, 1 jam, 4 jam) belum tersedia, jadi strategi ini cocok untuk \
swing trading harian, bukan scalping.";
use crate::clients::ai_router::ChatMessage;
use crate::services::screener_rules::model::UserScreenerRule;
use crate::services::screener_rules::types::{RuleScore, ScreenerRuleInput};

/// Body `PUT /api/screeners` — simpan strategi user (dibuat kalau belum ada, kalau sudah ada
/// seluruh isinya termasuk daftar rule diganti).
#[derive(Debug, Deserialize, Validate)]
pub struct ScreenerInput {
    #[validate(length(min = 1, max = 100, message = "Nama strategi wajib diisi (maksimal 100 karakter)"))]
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    /// Jumlah coin yang ditampilkan, 7–20 (default 10).
    #[serde(default)]
    pub max_results: Option<i16>,
    /// Skor minimal coin yang ditampilkan, 0–100 (default 0).
    #[serde(default)]
    pub min_score: Option<i16>,
    #[serde(default)]
    pub is_active: Option<bool>,
    pub rules: Vec<ScreenerRuleInput>,
}

/// Strategi + rule-nya.
#[derive(Debug, Serialize)]
pub struct Screener {
    #[serde(flatten)]
    pub screener: UserScreener,
    pub rules: Vec<UserScreenerRule>,
    /// Selalu `1D` — lihat `TIMEFRAME_NOTE`.
    pub timeframe: &'static str,
    pub timeframe_note: &'static str,
}

impl Screener {
    pub fn new(screener: UserScreener, rules: Vec<UserScreenerRule>) -> Self {
        Self { screener, rules, timeframe: TIMEFRAME, timeframe_note: TIMEFRAME_NOTE }
    }
}

/// Hasil `GET /api/screeners/results` — dihitung saat diminta, tidak disimpan.
#[derive(Debug, Serialize)]
pub struct ScreeningResult {
    pub screener_id: Uuid,
    pub name: String,
    /// Tanggal candle harian (UTC) terakhir yang dipakai menilai.
    pub candle_date: Option<NaiveDate>,
    /// Jumlah coin yang dinilai (coin dengan histori candle kurang tidak ikut).
    pub coins_screened: usize,
    pub coins: Vec<ScreenedCoin>,
    pub timeframe: &'static str,
    pub timeframe_note: &'static str,
}

#[derive(Debug, Serialize)]
pub struct ScreenedCoin {
    pub rank: usize,
    pub flex_param_id: Uuid,
    pub symbol: String,
    pub photo_url: Option<String>,
    /// Harga close candle harian terakhir.
    pub close: Decimal,
    /// Skor total 0–100 (rata-rata skor rule dibobot `weight`); 100 = cocok semua.
    pub score: f64,
    pub rules: Vec<RuleScore>,
}

/// Body `POST /api/screeners/ai-chat` — seluruh riwayat chat (dipegang FE), pesan terakhir dari
/// user. `role`: `user` | `assistant` (isi `assistant` = `reply` dari respons sebelumnya).
#[derive(Debug, Deserialize)]
pub struct AiChatInput {
    pub messages: Vec<ChatMessage>,
}

/// Balasan asisten AI. `draft` terisi saat AI sudah menyimpulkan strategi — sudah divalidasi &
/// formatnya sama dengan body `PUT /api/screeners`, jadi tombol Simpan cukup mengirimnya apa adanya.
#[derive(Debug, Serialize)]
pub struct AiChatResult {
    pub reply: String,
    pub draft: Option<ScreenerDraft>,
    /// Draf dari AI yang tetap tidak valid setelah diminta memperbaiki — tombol Simpan jangan
    /// ditampilkan kalau tidak kosong.
    pub draft_errors: Vec<String>,
    /// `true` = user sudah punya strategi, menyimpan draf akan MENGGANTI strategi itu (FE bisa
    /// menampilkan "Simpan & ganti strategi lama").
    pub replaces_existing: bool,
    /// Keterangan tetap untuk ditampilkan di layar chat (mis. di bawah judul).
    pub timeframe_note: &'static str,
}

#[derive(Debug, Serialize)]
pub struct ScreenerDraft {
    pub name: String,
    pub description: Option<String>,
    pub max_results: i16,
    pub min_score: i16,
    pub rules: Vec<DraftRule>,
}

/// Rule draf: field `PUT /api/screeners` + `indicator_code` & `summary` untuk ditampilkan
/// (diabaikan saat disimpan).
#[derive(Debug, Serialize)]
pub struct DraftRule {
    pub flex_param_id: Uuid,
    pub indicator_code: String,
    pub params: Value,
    pub condition: String,
    pub weight: i16,
    /// Penjelasan singkat dari AI, mis. "RSI 14 di bawah 30 — coin sedang jenuh jual".
    pub summary: String,
}
