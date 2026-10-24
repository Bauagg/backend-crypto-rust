//! EMA — Exponential Moving Average (`ta.ema`): seperti SMA tapi bobot data terbaru lebih besar,
//! jadi lebih cepat mengikuti perubahan harga. Default umum: 9 / 21 / 50 / 200.

pub const DEFAULT_PERIOD: usize = 21;

/// EMA dengan pengali `2 / (period + 1)`, diawali SMA `period` nilai pertama.
pub fn ema(values: &[f64], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; values.len()];
    if period == 0 || values.len() < period {
        return out;
    }
    let alpha = 2.0 / (period as f64 + 1.0);
    let mut value = values[..period].iter().sum::<f64>() / period as f64;
    out[period - 1] = Some(value);
    for i in period..values.len() {
        value += alpha * (values[i] - value);
        out[i] = Some(value);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indicator::test_util::approx;

    #[test]
    fn diawali_sma_lalu_eksponensial() {
        // period 3 -> alpha 0.5; awal = SMA(2,4,6) = 4
        let out = ema(&[2.0, 4.0, 6.0, 8.0, 10.0], 3);
        assert_eq!(out[1], None);
        approx(out[2], 4.0);
        approx(out[3], 6.0); // 4 + 0.5 × (8 − 4)
        approx(out[4], 8.0); // 6 + 0.5 × (10 − 6)
    }
}
