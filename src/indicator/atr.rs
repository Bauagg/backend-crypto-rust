//! ATR — Average True Range (`ta.atr`): rata-rata jarak gerak harga per hari (volatilitas).
//! Makin besar = harga makin liar; berguna untuk jarak stop loss. `atr_percent` = ATR ÷ close,
//! supaya bisa dibandingkan antar coin yang harganya beda jauh.

use super::{rma, Candle};

pub const DEFAULT_PERIOD: usize = 14;

/// True range: jarak terbesar antara high−low, |high − close kemarin|, |low − close kemarin|.
pub fn true_range(candles: &[Candle]) -> Vec<f64> {
    candles
        .iter()
        .enumerate()
        .map(|(i, c)| match i.checked_sub(1).map(|p| candles[p].close) {
            Some(prev_close) => (c.high - c.low)
                .max((c.high - prev_close).abs())
                .max((c.low - prev_close).abs()),
            None => c.high - c.low,
        })
        .collect()
}

/// ATR = rata-rata Wilder dari true range.
pub fn atr(candles: &[Candle], period: usize) -> Vec<Option<f64>> {
    rma(&true_range(candles), period)
}

/// ATR dalam persen dari harga close.
pub fn atr_percent(candles: &[Candle], period: usize) -> Vec<Option<f64>> {
    atr(candles, period)
        .into_iter()
        .zip(candles)
        .map(|(atr, c)| atr.filter(|_| c.close != 0.0).map(|a| a / c.close * 100.0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indicator::test_util::{approx, candle};

    #[test]
    fn true_range_memperhitungkan_gap() {
        let candles = [
            candle(10.0, 8.0, 9.0, 1.0),
            candle(15.0, 12.0, 14.0, 1.0), // gap naik: high − close kemarin = 6
            candle(14.0, 13.0, 13.5, 1.0),
        ];
        assert_eq!(true_range(&candles), vec![2.0, 6.0, 1.0]);
        let out = atr(&candles, 2);
        approx(out[1], 4.0);
        approx(out[2], (4.0 + 1.0) / 2.0);
        approx(atr_percent(&candles, 2)[2], 2.5 / 13.5 * 100.0);
    }
}
