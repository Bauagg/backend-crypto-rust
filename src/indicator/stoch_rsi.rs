//! Stochastic RSI (`ta.stoch` dari `ta.rsi`): Stochastic yang dihitung dari nilai RSI, bukan
//! harga — lebih cepat bereaksi daripada RSI biasa. 0–100; di atas 80 = jenuh beli, di bawah 20 =
//! jenuh jual. %K memotong ke atas %D di area jenuh jual = tanda naik.

use super::rsi::rsi;
use super::sma::sma;
use super::on_defined;

pub const DEFAULT_RSI_PERIOD: usize = 14;
pub const DEFAULT_STOCH_PERIOD: usize = 14;
pub const DEFAULT_K_SMOOTHING: usize = 3;
pub const DEFAULT_D_PERIOD: usize = 3;
pub const OVERSOLD: f64 = 20.0;

#[derive(Debug, Clone, PartialEq)]
pub struct StochRsi {
    pub k: Vec<Option<f64>>,
    pub d: Vec<Option<f64>>,
}

pub fn stoch_rsi(
    closes: &[f64],
    rsi_period: usize,
    stoch_period: usize,
    k_smoothing: usize,
    d_period: usize,
) -> StochRsi {
    let rsi_values = rsi(closes, rsi_period);
    let raw = on_defined(&rsi_values, |values| {
        let mut out = vec![None; values.len()];
        if stoch_period == 0 {
            return out;
        }
        for i in stoch_period.saturating_sub(1)..values.len() {
            let window = &values[i + 1 - stoch_period..=i];
            let highest = window.iter().copied().fold(f64::MIN, f64::max);
            let lowest = window.iter().copied().fold(f64::MAX, f64::min);
            out[i] = Some(if highest > lowest {
                (values[i] - lowest) / (highest - lowest) * 100.0
            } else {
                50.0
            });
        }
        out
    });
    let k = on_defined(&raw, |values| sma(values, k_smoothing.max(1)));
    let d = on_defined(&k, |values| sma(values, d_period.max(1)));
    StochRsi { k, d }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indicator::test_util::approx;

    #[test]
    fn posisi_rsi_di_rentangnya() {
        // naik 10 hari lalu turun -> RSI di puncak saat naik, merosot saat turun
        let closes: Vec<f64> = (1..=10).chain((1..=6).rev()).map(f64::from).collect();
        let out = stoch_rsi(&closes, 3, 3, 1, 1);
        // warm-up: RSI mulai index 3, stoch 3 nilai RSI -> mulai index 5
        assert_eq!(out.k[4], None);
        assert!(out.k[5].is_some());
        approx(*out.k.last().unwrap(), 0.0); // RSI sedang di titik terendah rentangnya
    }
}
