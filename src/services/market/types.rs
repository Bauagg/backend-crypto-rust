use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::clients::binance;

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
pub struct RecommendationQuery {
    /// Jumlah coin, 5-10 (default 10). Peringkat 1-5 = UTAMA, 6-10 = PELENGKAP.
    pub limit: Option<u8>,
    /// `YYYY-MM-DD` untuk melihat rekomendasi hari tertentu; kosong = candle harian terakhir.
    pub date: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct SymbolsQuery {
    /// Cari berdasarkan symbol, partial match (contoh: search=BTC cocok dengan BTCUSDT).
    pub search: Option<String>,
    pub page: Option<i64>,
    pub limit: Option<i64>,
}

/// Satu pair yang ditampilkan di FE: identitas & foto berasal dari `flex_params` (database kita,
/// diedit lewat endpoint flex-params yang sudah ada), harga & persen naik/turun dari exchange real-time.
#[derive(Debug, Serialize)]
pub struct MarketSymbol {
    pub symbol: String,
    pub photo_url: Option<String>,
    pub last_price: String,
    pub price_change_percent: String,
}

/// Harga terakhir & persen naik/turun 24 jam 1 pair — isi memori `TickerHub` sekaligus bentuk
/// pesan WebSocket `/ws/tickers`.
#[derive(Debug, Clone, Serialize)]
pub struct MarketTicker {
    pub symbol: String,
    pub last_price: String,
    pub price_change_percent: String,
}

impl MarketTicker {
    /// `None` kalau angka dari exchange tidak bisa dibaca.
    pub fn from_mini_ticker(event: binance::MiniTickerEvent) -> Option<Self> {
        let close: Decimal = event.close.parse().ok()?;
        let open: Decimal = event.open.parse().ok()?;
        let percent = if open.is_zero() {
            Decimal::ZERO
        } else {
            (close - open) / open * Decimal::ONE_HUNDRED
        };
        Some(Self {
            symbol: event.symbol,
            last_price: event.close,
            price_change_percent: percent.round_dp(2).to_string(),
        })
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct WsTickersQuery {
    /// Simbol awal yang dipantau, dipisah koma (opsional): `BTCUSDT,ETHUSDT`.
    pub symbols: Option<String>,
}

/// Pesan dari client WebSocket `/ws/tickers` untuk mengganti daftar simbol yang dipantau,
/// mis. setelah scroll: `{"symbols":["BTCUSDT","ETHUSDT"]}`.
#[derive(Debug, Deserialize)]
pub struct WsTickersRequest {
    pub symbols: Vec<String>,
}

/// Satu candle OHLCV, bentuknya sama baik dari REST (histori) maupun WebSocket (live) —
/// supaya frontend cuma perlu satu struktur data untuk keduanya. `Deserialize` dibutuhkan
/// untuk baca balik dari cache Redis di `get_klines_service`.
#[derive(Debug, Clone, Serialize, Deserialize)]
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

/// Dari REST `/klines` — histori, ditandai closed.
impl From<binance::Kline> for Candle {
    fn from(kline: binance::Kline) -> Self {
        Candle {
            open_time: kline.open_time,
            open: kline.open,
            high: kline.high,
            low: kline.low,
            close: kline.close,
            volume: kline.volume,
            close_time: kline.close_time,
            is_closed: true,
        }
    }
}

/// Dari stream WebSocket — candle live (atau event terakhir saat candle close).
impl From<binance::KlineEvent> for Candle {
    fn from(event: binance::KlineEvent) -> Self {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn event(close: &str, open: &str) -> binance::MiniTickerEvent {
        binance::MiniTickerEvent {
            symbol: "BTCUSDT".to_string(),
            close: close.to_string(),
            open: open.to_string(),
        }
    }

    #[test]
    fn mini_ticker_percent_matches_24h_change() {
        let up = MarketTicker::from_mini_ticker(event("110.00000000", "100.00000000")).unwrap();
        assert_eq!(up.last_price, "110.00000000");
        assert_eq!(up.price_change_percent, "10.00");

        let down = MarketTicker::from_mini_ticker(event("86150", "87000")).unwrap();
        assert_eq!(down.price_change_percent, "-0.98");
    }

    #[test]
    fn mini_ticker_handles_zero_open_and_bad_numbers() {
        let zero = MarketTicker::from_mini_ticker(event("1", "0")).unwrap();
        assert_eq!(zero.price_change_percent, "0");
        assert!(MarketTicker::from_mini_ticker(event("abc", "1")).is_none());
    }
}
