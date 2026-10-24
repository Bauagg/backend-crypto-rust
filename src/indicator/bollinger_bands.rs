//! Bollinger Bands (`ta.bb`): pita harga = SMA ± `multiplier` × standar deviasi.
//! Harga menyentuh pita atas = relatif mahal, pita bawah = relatif murah; pita menyempit
//! (`bandwidth` kecil) = harga biasanya akan bergerak besar.

use super::sma::sma;

pub const DEFAULT_PERIOD: usize = 20;
pub const DEFAULT_MULTIPLIER: f64 = 2.0;

#[derive(Debug, Clone, PartialEq)]
pub struct BollingerBands {
    pub middle: Vec<Option<f64>>,
    pub upper: Vec<Option<f64>>,
    pub lower: Vec<Option<f64>>,
    /// Posisi harga di dalam pita: 0 = di pita bawah, 1 = di pita atas (bisa < 0 / > 1).
    pub percent_b: Vec<Option<f64>>,
    /// Lebar pita relatif terhadap tengah: (atas − bawah) / tengah.
    pub bandwidth: Vec<Option<f64>>,
}

pub fn bollinger_bands(closes: &[f64], period: usize, multiplier: f64) -> BollingerBands {
    let middle = sma(closes, period);
    let len = closes.len();
    let (mut upper, mut lower) = (vec![None; len], vec![None; len]);
    let (mut percent_b, mut bandwidth) = (vec![None; len], vec![None; len]);

    for i in 0..len {
        let Some(mean) = middle[i] else { continue };
        let window = &closes[i + 1 - period..=i];
        // Standar deviasi populasi (dibagi `period`), sama dengan TradingView.
        let variance = window.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / period as f64;
        let deviation = multiplier * variance.sqrt();
        let (up, low) = (mean + deviation, mean - deviation);
        upper[i] = Some(up);
        lower[i] = Some(low);
        percent_b[i] = (up > low).then(|| (closes[i] - low) / (up - low));
        bandwidth[i] = (mean != 0.0).then(|| (up - low) / mean);
    }
    BollingerBands { middle, upper, lower, percent_b, bandwidth }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indicator::test_util::approx;

    #[test]
    fn pita_dari_standar_deviasi() {
        // 2,4,4,4,5,5,7,9: rata-rata 5, std dev populasi 2
        let closes = [2.0, 4.0, 4.0, 4.0, 5.0, 5.0, 7.0, 9.0];
        let out = bollinger_bands(&closes, 8, 2.0);
        approx(out.middle[7], 5.0);
        approx(out.upper[7], 9.0);
        approx(out.lower[7], 1.0);
        approx(out.percent_b[7], 1.0); // close 9 tepat di pita atas
        approx(out.bandwidth[7], 8.0 / 5.0);
        assert_eq!(out.upper[6], None);
    }
}
