//! Stochastic Oscillator (`ta.stoch`): posisi close di antara harga terendah & tertinggi
//! `k_period` hari terakhir, 0–100. Di atas 80 = jenuh beli, di bawah 20 = jenuh jual.
//! %K memotong ke atas %D di area jenuh jual = tanda naik.

use super::sma::sma;
use super::{on_defined, Candle};

pub const DEFAULT_K_PERIOD: usize = 14;
pub const DEFAULT_K_SMOOTHING: usize = 3;
pub const DEFAULT_D_PERIOD: usize = 3;
pub const OVERSOLD: f64 = 20.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Stochastic {
    /// %K (sudah dihaluskan SMA `k_smoothing`; `k_smoothing = 1` = %K mentah / fast stochastic).
    pub k: Vec<Option<f64>>,
    /// %D = SMA %K.
    pub d: Vec<Option<f64>>,
}

pub fn stochastic(candles: &[Candle], k_period: usize, k_smoothing: usize, d_period: usize) -> Stochastic {
    let mut raw = vec![None; candles.len()];
    if k_period > 0 {
        for i in k_period.saturating_sub(1)..candles.len() {
            let window = &candles[i + 1 - k_period..=i];
            let highest = window.iter().map(|c| c.high).fold(f64::MIN, f64::max);
            let lowest = window.iter().map(|c| c.low).fold(f64::MAX, f64::min);
            raw[i] = Some(if highest > lowest {
                (candles[i].close - lowest) / (highest - lowest) * 100.0
            } else {
                // Harga tidak bergerak sama sekali — anggap di tengah.
                50.0
            });
        }
    }
    let k = on_defined(&raw, |values| sma(values, k_smoothing.max(1)));
    let d = on_defined(&k, |values| sma(values, d_period.max(1)));
    Stochastic { k, d }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indicator::test_util::{approx, candle};

    #[test]
    fn posisi_close_di_rentang_harga() {
        let candles = [
            candle(10.0, 0.0, 5.0, 1.0),
            candle(10.0, 0.0, 10.0, 1.0),
            candle(10.0, 0.0, 0.0, 1.0),
            candle(10.0, 0.0, 10.0, 1.0),
        ];
        let fast = stochastic(&candles, 2, 1, 2);
        approx(fast.k[1], 100.0);
        approx(fast.k[2], 0.0);
        approx(fast.d[2], 50.0);

        let slow = stochastic(&candles, 2, 2, 1);
        assert_eq!(slow.k[1], None);
        approx(slow.k[2], 50.0); // rata-rata 100 & 0
    }
}
