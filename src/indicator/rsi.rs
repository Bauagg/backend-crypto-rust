//! RSI — Relative Strength Index (`ta.rsi`): kekuatan kenaikan vs penurunan harga, 0–100.
//! Di atas 70 = jenuh beli (rawan turun), di bawah 30 = jenuh jual (berpeluang naik).

use super::rma;

pub const DEFAULT_PERIOD: usize = 14;
pub const OVERSOLD: f64 = 30.0;

/// RSI Wilder dari harga close. Nilai pertama ada di index `period` (butuh `period` selisih).
pub fn rsi(closes: &[f64], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; closes.len()];
    if period == 0 || closes.len() <= period {
        return out;
    }
    let (gains, losses): (Vec<f64>, Vec<f64>) = closes
        .windows(2)
        .map(|w| {
            let change = w[1] - w[0];
            (change.max(0.0), (-change).max(0.0))
        })
        .unzip();

    let avg_gain = rma(&gains, period);
    let avg_loss = rma(&losses, period);
    for i in 0..gains.len() {
        if let (Some(gain), Some(loss)) = (avg_gain[i], avg_loss[i]) {
            out[i + 1] = Some(if loss == 0.0 {
                if gain == 0.0 { 50.0 } else { 100.0 }
            } else {
                100.0 - 100.0 / (1.0 + gain / loss)
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indicator::test_util::approx;

    #[test]
    fn naik_terus_100_turun_terus_0() {
        let up: Vec<f64> = (1..=20).map(f64::from).collect();
        let down: Vec<f64> = up.iter().rev().copied().collect();
        approx(*rsi(&up, 14).last().unwrap(), 100.0);
        approx(*rsi(&down, 14).last().unwrap(), 0.0);
    }

    #[test]
    fn hitungan_wilder() {
        // selisih: +1, −1, +2 -> period 2: gain awal (1+0)/2 = 0.5, loss awal (0+1)/2 = 0.5
        // lalu gain = (0.5×1 + 2)/2 = 1.25, loss = (0.5×1 + 0)/2 = 0.25 -> RS 5 -> RSI 83.333
        let out = rsi(&[10.0, 11.0, 10.0, 12.0], 2);
        assert_eq!(out[1], None);
        approx(out[2], 50.0);
        approx(out[3], 100.0 - 100.0 / 6.0);
    }
}
