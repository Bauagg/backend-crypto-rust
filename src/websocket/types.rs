use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::clients::binance;

/// Query `WS /api/market/ws` (chart 1 coin).
#[derive(Debug, Deserialize)]
pub struct WsKlineQuery {
    pub symbol: String,
    #[serde(default)]
    pub interval: Option<String>,
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

/// Query `WS /api/market/ws/tickers` (daftar coin).
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
