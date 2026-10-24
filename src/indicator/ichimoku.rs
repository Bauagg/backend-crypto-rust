//! Ichimoku Cloud: tren + "awan" support/resistance sekaligus.
//! - Tenkan-sen = (high tertinggi + low terendah) / 2 selama 9 candle (garis cepat).
//! - Kijun-sen = sama untuk 26 candle (garis lambat).
//! - Senkou A = (Tenkan + Kijun) / 2, Senkou B = titik tengah 52 candle — keduanya digeser ke
//!   depan, membentuk awan (kumo).
//!
//! Harga di atas awan = tren naik, di bawah awan = tren turun, di dalam awan = ragu.
//! Tenkan memotong ke atas Kijun = tanda naik.

use super::Candle;

pub const DEFAULT_TENKAN: usize = 9;
pub const DEFAULT_KIJUN: usize = 26;
pub const DEFAULT_SENKOU_B: usize = 52;
pub const DEFAULT_DISPLACEMENT: usize = 26;

#[derive(Debug, Clone, PartialEq)]
pub struct Ichimoku {
    pub tenkan: Vec<Option<f64>>,
    pub kijun: Vec<Option<f64>>,
    /// Batas awan yang BERLAKU di candle ke-i (sudah digeser): dihitung dari candle
    /// `i − (displacement − 1)`, sama dengan yang tampil di chart TradingView pada candle itu.
    pub senkou_a: Vec<Option<f64>>,
    pub senkou_b: Vec<Option<f64>>,
}

impl Ichimoku {
    /// Batas atas & bawah awan di candle ke-i.
    pub fn cloud_at(&self, i: usize) -> Option<(f64, f64)> {
        let (a, b) = (self.senkou_a.get(i).copied()??, self.senkou_b.get(i).copied()??);
        Some((a.max(b), a.min(b)))
    }
}

/// Titik tengah high tertinggi & low terendah `period` candle terakhir.
fn midpoint(candles: &[Candle], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; candles.len()];
    if period == 0 {
        return out;
    }
    for i in period.saturating_sub(1)..candles.len() {
        let window = &candles[i + 1 - period..=i];
        let high = window.iter().map(|c| c.high).fold(f64::MIN, f64::max);
        let low = window.iter().map(|c| c.low).fold(f64::MAX, f64::min);
        out[i] = Some((high + low) / 2.0);
    }
    out
}

pub fn ichimoku(
    candles: &[Candle],
    tenkan_period: usize,
    kijun_period: usize,
    senkou_b_period: usize,
    displacement: usize,
) -> Ichimoku {
    let tenkan = midpoint(candles, tenkan_period);
    let kijun = midpoint(candles, kijun_period);
    let lead_a: Vec<Option<f64>> =
        tenkan.iter().zip(&kijun).map(|(t, k)| Some(((*t)? + (*k)?) / 2.0)).collect();
    let lead_b = midpoint(candles, senkou_b_period);

    let shift = displacement.saturating_sub(1);
    let shifted = |values: &[Option<f64>]| -> Vec<Option<f64>> {
        (0..values.len()).map(|i| i.checked_sub(shift).and_then(|j| values[j])).collect()
    };
    Ichimoku {
        senkou_a: shifted(&lead_a),
        senkou_b: shifted(&lead_b),
        tenkan,
        kijun,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indicator::test_util::{approx, candle};

    #[test]
    fn garis_dan_awan_digeser() {
        let candles: Vec<Candle> =
            (0..10).map(|i| candle(f64::from(i) + 2.0, f64::from(i), f64::from(i) + 1.0, 1.0)).collect();
        let out = ichimoku(&candles, 2, 3, 4, 3);
        // tenkan index 1: high max 3, low min 0 -> 1.5
        approx(out.tenkan[1], 1.5);
        // kijun index 2: high max 4, low min 0 -> 2
        approx(out.kijun[2], 2.0);
        // senkou A pertama dihitung di index 2 ((2.5 + 2) / 2), tampil 2 candle kemudian
        assert_eq!(out.senkou_a[3], None);
        approx(out.senkou_a[4], 2.25);
        // senkou B pertama di index 3 (high 5, low 0 -> 2.5), tampil di index 5
        approx(out.senkou_b[5], 2.5);
        let (top, bottom) = out.cloud_at(9).unwrap();
        assert!(top >= bottom);
    }
}
