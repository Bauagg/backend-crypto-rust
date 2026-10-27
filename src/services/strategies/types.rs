use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;
use validator::Validate;

use super::model::Strategy;
use crate::clients::ai_router::ChatMessage;
use crate::services::strategy_backtests::types::BacktestSummary;
use crate::services::transactions::types::LivePerformance;
use crate::services::strategy_versions::types::{BotConfig, BotConfigInput, VersionInput, VersionView};
use crate::strategy_engine::inputs::InputSpec;

/// Keterangan data untuk ditampilkan FE di hasil strategi.
pub const TIMEFRAME_NOTE: &str = "Strategi dijalankan pada data candle harian (OHLC timeframe 1D / D1). \
Timeframe kecil belum tersedia, jadi cocok untuk swing trading harian, bukan scalping.";

/// Body `POST /api/strategies` — strategi baru + isi versi pertamanya.
#[derive(Debug, Deserialize, Validate)]
pub struct CreateStrategyInput {
    #[validate(length(min = 1, max = 100, message = "Nama strategi wajib diisi (maksimal 100 karakter)"))]
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    /// `SCREENER` | `BOT` (default `SCREENER`).
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(flatten)]
    pub version: VersionInput,
}

/// Body `PUT /api/strategies/:id` — ubah identitas/status (isi script lewat versi baru).
#[derive(Debug, Deserialize, Validate)]
pub struct UpdateStrategyInput {
    #[validate(length(min = 1, max = 100, message = "Nama strategi wajib diisi (maksimal 100 karakter)"))]
    pub name: Option<String>,
    pub description: Option<String>,
    pub kind: Option<String>,
    /// `DRAFT` | `ACTIVE` | `ARCHIVED`. (`visibility` diatur otomatis oleh Store.)
    pub status: Option<String>,
}

/// Strategi + versi yang dipakai. `current_version.source_code` hanya untuk pemilik.
#[derive(Debug, Serialize)]
pub struct StrategyDetail {
    #[serde(flatten)]
    pub strategy: Strategy,
    pub is_owner: bool,
    pub current_version: Option<VersionView>,
    /// Hanya di `GET /strategies/:id`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stats: Option<StrategyStats>,
}

/// Statistik strategi — dihitung sistem, sama di detail strategi & Store.
#[derive(Debug, Serialize)]
pub struct StrategyStats {
    /// Backtest resmi versi aktif (dijalankan pemilik, setelan default). `None` = belum dibacktest.
    pub official_backtest: Option<BacktestSummary>,
    /// Kurva modal & drawdown harian backtest resmi: `[{"date", "equity", "drawdown_percent", "btc_equity"}]`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub equity_curve: Option<Value>,
    /// Performa transaksi asli semua pemakai (semua versi), DEMO & LIVE dipisah.
    pub live_performance: Vec<LivePerformance>,
    /// Performa transaksi asli user yang melihat, DEMO & LIVE dipisah.
    pub my_performance: Vec<LivePerformance>,
}

/// Body `POST /api/strategies/validate` — coba script tanpa menyimpan.
#[derive(Debug, Deserialize)]
pub struct ValidateScriptInput {
    pub source_code: String,
    #[serde(default)]
    pub inputs: Value,
    /// Nilai setelan untuk percobaan (kosong = default).
    #[serde(default)]
    pub input_values: Value,
    /// Aturan robot & cakupan coin (kosong = default) — script dicoba ke coin dalam cakupan.
    #[serde(flatten)]
    pub bot: BotConfigInput,
}

#[derive(Debug, Serialize)]
pub struct ValidateScriptResult {
    pub valid: bool,
    /// Error compile / error saat dijalankan (contoh dari beberapa coin).
    pub errors: Vec<String>,
    pub inputs: Vec<InputSpec>,
    pub bot: Option<BotConfig>,
    pub run: Option<StrategyRun>,
}

/// Query `GET /api/strategies/:id/results` & `GET /api/strategy-subscriptions/:id/results`.
#[derive(Debug, Default, Deserialize)]
pub struct RunQuery {
    /// Jumlah coin skor tertinggi yang ditampilkan, 1–50 (default 10).
    pub limit: Option<usize>,
}

/// Hasil menjalankan 1 script ke semua coin aktif (tidak disimpan).
#[derive(Debug, Serialize)]
pub struct StrategyRun {
    pub strategy_id: Option<Uuid>,
    pub strategy_version_id: Option<Uuid>,
    pub version_no: Option<i32>,
    /// Nilai setelan yang dipakai.
    pub input_values: Value,
    pub candle_date: Option<NaiveDate>,
    /// Coin yang berhasil dinilai.
    pub coins_screened: usize,
    /// Coin yang gagal dinilai (mis. histori kurang) + contoh errornya.
    pub coins_failed: usize,
    pub sample_errors: Vec<String>,
    pub coins: Vec<StrategyCoin>,
    pub timeframe: &'static str,
    pub timeframe_note: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct StrategyCoin {
    pub rank: usize,
    pub flex_param_id: Uuid,
    pub symbol: String,
    pub photo_url: Option<String>,
    /// Harga close candle harian terakhir.
    pub close: Decimal,
    /// 0–100 dari script.
    pub score: f64,
    pub reason: String,
}

/// Body `POST /api/strategies/ai-chat`.
#[derive(Debug, Deserialize)]
pub struct StrategyAiChatInput {
    pub messages: Vec<ChatMessage>,
    /// Isi = AI mengubah strategi milik user ini (script versi aktif ikut dikirim ke AI).
    #[serde(default)]
    pub strategy_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct StrategyAiChatResult {
    pub reply: String,
    /// Strategi yang disimpulkan AI — sudah lolos compile & dicoba ke semua coin. Simpan dengan
    /// `POST /api/strategies` (baru) atau `POST /api/strategies/:id/versions` (versi baru).
    pub draft: Option<StrategyDraft>,
    pub draft_errors: Vec<String>,
    pub timeframe_note: &'static str,
}

#[derive(Debug, Serialize)]
pub struct StrategyDraft {
    pub name: String,
    pub description: Option<String>,
    pub kind: String,
    pub source_code: String,
    pub inputs: Value,
    pub change_note: Option<String>,
    #[serde(flatten)]
    pub bot: BotConfig,
    /// Hasil percobaan draf ke coin dalam cakupan (5 teratas).
    pub preview: Vec<StrategyCoin>,
}
