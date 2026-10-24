//! SMA — Simple Moving Average: rata-rata `period` nilai terakhir (`ta.sma`).
//! Default umum: 20 / 50 / 100 / 200. Harga di atas SMA = tren naik.

/// Default screener: SMA 50 (tren menengah).
pub const DEFAULT_PERIOD: usize = 50;

/// SMA dari deret nilai (biasanya harga close). `None` sampai ada `period` nilai.
pub fn sma(values: &[f64], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; values.len()];
    if period == 0 || values.len() < period {
        return out;
    }
    let mut sum: f64 = values[..period].iter().sum();
    out[period - 1] = Some(sum / period as f64);
    for i in period..values.len() {
        sum += values[i] - values[i - period];
        out[i] = Some(sum / period as f64);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indicator::test_util::approx;

    #[test]
    fn rata_rata_bergerak() {
        let out = sma(&[1.0, 2.0, 3.0, 4.0, 5.0], 3);
        assert_eq!(out[..2], [None, None]);
        approx(out[2], 2.0);
        approx(out[3], 3.0);
        approx(out[4], 4.0);
    }

    #[test]
    fn data_kurang_semua_kosong() {
        assert!(sma(&[1.0, 2.0], 3).iter().all(Option::is_none));
    }
}
