//! OBV — On-Balance Volume (`ta.obv`): volume kumulatif — ditambah saat close naik, dikurangi
//! saat close turun. OBV naik saat harga datar/turun = akumulasi (pembeli diam-diam masuk).
//! Yang dilihat arah/kemiringannya, bukan angka mutlaknya.

use super::sma::sma;
use super::Candle;

/// Periode garis rata-rata OBV untuk melihat arahnya (OBV di atas rata-ratanya = sedang naik).
pub const DEFAULT_SIGNAL_PERIOD: usize = 20;

/// OBV mulai 0 di candle pertama.
pub fn obv(candles: &[Candle]) -> Vec<f64> {
    let mut total = 0.0;
    candles
        .iter()
        .enumerate()
        .map(|(i, c)| {
            if let Some(prev) = i.checked_sub(1).map(|p| candles[p].close) {
                if c.close > prev {
                    total += c.volume;
                } else if c.close < prev {
                    total -= c.volume;
                }
            }
            total
        })
        .collect()
}

/// Rata-rata OBV `period` candle — pembanding arah OBV.
pub fn obv_signal(candles: &[Candle], period: usize) -> Vec<Option<f64>> {
    sma(&obv(candles), period)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indicator::test_util::{approx, candle};

    #[test]
    fn volume_ikut_arah_harga() {
        let candles = [
            candle(1.0, 1.0, 10.0, 100.0),
            candle(1.0, 1.0, 11.0, 50.0), // naik +50
            candle(1.0, 1.0, 9.0, 30.0),  // turun −30
            candle(1.0, 1.0, 9.0, 99.0),  // sama, tidak berubah
        ];
        assert_eq!(obv(&candles), vec![0.0, 50.0, 20.0, 20.0]);
        approx(obv_signal(&candles, 2)[3], 20.0);
    }
}
