use serde::{Deserialize, Serialize};

pub const ALLOWED_INTERVALS: [&str; 15] = [
    "1m", "3m", "5m", "15m", "30m", "1h", "2h", "4h", "6h", "8h", "12h", "1d", "3d", "1w", "1M",
];

pub fn is_valid_interval(interval: &str) -> bool {
    ALLOWED_INTERVALS.contains(&interval)
}

#[derive(Debug, Deserialize)]
pub struct KlinesQuery {
    pub symbol: String,
    #[serde(default)]
    pub interval: Option<String>,
    #[serde(default)]
    pub limit: Option<u16>,
    /// Untuk pagination geser-ke-kiri: isi dengan `open_time` candle paling lama yang sudah
    /// dimiliki FE, dapat balik candle-candle SEBELUM waktu itu (histori lebih lama).
    #[serde(default)]
    pub end_time: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct WsKlineQuery {
    pub symbol: String,
    #[serde(default)]
    pub interval: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct SymbolsQuery {
    /// Cari berdasarkan symbol, partial match (contoh: search=BTC cocok dengan BTCUSDT).
    pub search: Option<String>,
    pub page: Option<i64>,
    pub limit: Option<i64>,
}

/// Bentuk mentah respons endpoint /api/v3/ticker/24hr milik Tokocrypto — dipakai untuk
/// ambil harga terkini & persentase naik/turun 24 jam per simbol.
#[derive(Debug, Deserialize)]
pub struct Ticker24hr {
    #[serde(rename = "lastPrice")]
    pub last_price: String,
    #[serde(rename = "priceChangePercent")]
    pub price_change_percent: String,
}

/// Satu pair yang ditampilkan di FE: identitas & foto berasal dari `flex_params` (database kita,
/// diedit lewat endpoint flex-params yang sudah ada), harga & persen naik/turun dari Tokocrypto real-time.
#[derive(Debug, Serialize)]
pub struct MarketSymbol {
    pub symbol: String,
    pub photo_url: Option<String>,
    pub last_price: String,
    pub price_change_percent: String,
}

/// Satu candle OHLCV, bentuknya sama baik dari REST (histori) maupun WebSocket (live) —
/// supaya frontend cuma perlu satu struktur data untuk keduanya.
#[derive(Debug, Serialize)]
pub struct Candle {
    pub open_time: i64,
    pub open: String,
    pub high: String,
    pub low: String,
    pub close: String,
    pub volume: String,
    pub close_time: i64,
    /// `true` kalau candle ini sudah final/closed, `false` kalau masih berjalan (baru dari WebSocket).
    /// Selalu `true` untuk data dari REST karena itu candle historis yang sudah pasti closed.
    pub is_closed: bool,
}

/// Bentuk payload mentah kline event dari Tokocrypto WebSocket, dipakai untuk parsing internal saja.
#[derive(Debug, Deserialize)]
pub struct RawKlineEvent {
    pub k: RawKlineData,
}

#[derive(Debug, Deserialize)]
pub struct RawKlineData {
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
    #[serde(rename = "x")]
    pub is_closed: bool,
}

impl From<RawKlineEvent> for Candle {
    fn from(event: RawKlineEvent) -> Self {
        Candle {
            open_time: event.k.open_time,
            open: event.k.open,
            high: event.k.high,
            low: event.k.low,
            close: event.k.close,
            volume: event.k.volume,
            close_time: event.k.close_time,
            is_closed: event.k.is_closed,
        }
    }
}

/// Sinyal BUY terbaik, digabung dengan foto dari `flex_params` — siap ditampilkan FE.
/// Menyertakan seluruh rincian indikator dari API ML (bukan cuma verdict) supaya FE bisa
/// menampilkan alasan di balik sinyal, bukan cuma angka akhir.
#[derive(Debug, Serialize)]
pub struct TopSignal {
    pub symbol: String,
    pub photo_url: Option<String>,
    pub ml_confidence: f64,
    pub reason: String,
    pub close: f64,
    pub ma: MaDetail,
    pub momentum: MomentumDetail,
    pub volume: VolumeDetail,
    pub relative_strength: Option<f64>,
    pub reversal: ReversalDetail,
    pub regime: RegimeDetail,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MaDetail {
    pub ma_fast: Option<f64>,
    pub ma_slow: Option<f64>,
    pub ma_diff_pct: Option<f64>,
    pub trend_state: Option<String>,
    pub below_ma200: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MomentumDetail {
    pub rsi: Option<f64>,
    pub fng_value: Option<f64>,
    pub price_change_pct: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct VolumeDetail {
    pub volume_avg: Option<f64>,
    pub volume_ratio: Option<f64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ReversalDetail {
    pub bottoming: Option<bool>,
    pub topping: Option<bool>,
    pub reversal_verdict: Option<String>,
    pub reversal_confidence: Option<f64>,
    pub reversal_confirmed: Option<bool>,
    pub confirm_count: Option<i64>,
    pub confirm_higher_close: Option<bool>,
    pub confirm_break_high: Option<bool>,
    pub confirm_rsi_rising: Option<bool>,
    pub confirm_bullish_div: Option<bool>,
    pub confirm_not_falling: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RegimeDetail {
    pub regime_ok: Option<bool>,
    pub crashing: Option<bool>,
    pub price_stable: Option<bool>,
}

/// Satu bar OHLCV harian, dikirim ke API ML (`POST /score-live`) sebagai histori data live Tokocrypto.
#[derive(Debug, Serialize)]
pub struct OhlcvBar {
    pub date: String,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

/// Body request ke POST /score-live milik API strategi ML — skor sinyal untuk simbol APA PUN
/// (tidak terbatas ke coin yang modelnya sudah dilatih), asal disuplai histori OHLCV yang cukup.
#[derive(Debug, Serialize)]
pub struct ScoreLiveRequest {
    pub symbol: String,
    pub coin_ohlcv: Vec<OhlcvBar>,
    pub btc_close: Vec<f64>,
    pub fng: Vec<f64>,
}

#[derive(Debug, Deserialize)]
pub struct ScoreLiveResponse {
    pub symbol: String,
    #[allow(dead_code)]
    pub date: String,
    pub verdict: String,
    pub ml_confidence: Option<f64>,
    pub reason: String,
    pub close: f64,
    pub ma: MaDetail,
    pub momentum: MomentumDetail,
    pub volume: VolumeDetail,
    pub relative_strength: Option<f64>,
    pub reversal: ReversalDetail,
    pub regime: RegimeDetail,
    #[allow(dead_code)]
    pub bars_received: u32,
}
