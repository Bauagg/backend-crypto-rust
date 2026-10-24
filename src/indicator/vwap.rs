//! VWAP — Volume Weighted Average Price: harga rata-rata yang dibobot volume. Harga di atas VWAP
//! = pembeli lebih kuat, di bawah = penjual lebih kuat.
//!
//! VWAP klasik di-reset tiap hari dari data intraday; data kita hanya candle harian (D1), jadi
//! yang dihitung VWAP bergulir `period` hari: Σ(harga tipikal × volume) ÷ Σ volume, harga
//! tipikal = (high + low + close) / 3.

use super::Candle;

pub const DEFAULT_PERIOD: usize = 20;

pub fn vwap(candles: &[Candle], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; candles.len()];
    if period == 0 {
        return out;
    }
    for i in period.saturating_sub(1)..candles.len() {
        let window = &candles[i + 1 - period..=i];
        let volume: f64 = window.iter().map(|c| c.volume).sum();
        if volume > 0.0 {
            let weighted: f64 = window.iter().map(|c| (c.high + c.low + c.close) / 3.0 * c.volume).sum();
            out[i] = Some(weighted / volume);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indicator::test_util::{approx, candle};

    #[test]
    fn rata_rata_dibobot_volume() {
        let candles = [candle(10.0, 10.0, 10.0, 1.0), candle(20.0, 20.0, 20.0, 3.0)];
        let out = vwap(&candles, 2);
        assert_eq!(out[0], None);
        approx(out[1], (10.0 + 20.0 * 3.0) / 4.0);
    }
}
