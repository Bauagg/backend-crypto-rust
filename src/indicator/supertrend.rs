//! Supertrend (`ta.supertrend`): garis pengikut tren dari ATR. Harga di atas garis = tren naik
//! (garis jadi support), di bawah = tren turun (garis jadi resistance). Berganti arah = sinyal.

use super::atr::atr;
use super::Candle;

pub const DEFAULT_ATR_PERIOD: usize = 10;
pub const DEFAULT_MULTIPLIER: f64 = 3.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Supertrend {
    pub value: Vec<Option<f64>>,
    /// `Some(true)` = tren naik.
    pub uptrend: Vec<Option<bool>>,
}

pub fn supertrend(candles: &[Candle], atr_period: usize, multiplier: f64) -> Supertrend {
    let atr_values = atr(candles, atr_period);
    let len = candles.len();
    let mut value = vec![None; len];
    let mut uptrend = vec![None; len];

    let (mut prev_upper, mut prev_lower) = (0.0_f64, 0.0_f64);
    let mut prev_up: Option<bool> = None;
    for i in 0..len {
        let Some(range) = atr_values[i] else { continue };
        let c = candles[i];
        let mid = (c.high + c.low) / 2.0;
        let mut upper = mid + multiplier * range;
        let mut lower = mid - multiplier * range;

        let up = match (prev_up, i.checked_sub(1).map(|p| candles[p].close)) {
            (Some(was_up), Some(prev_close)) => {
                // Pita hanya boleh mengencang searah tren, kecuali harga sudah menembusnya.
                if !(lower > prev_lower || prev_close < prev_lower) {
                    lower = prev_lower;
                }
                if !(upper < prev_upper || prev_close > prev_upper) {
                    upper = prev_upper;
                }
                if was_up { c.close >= lower } else { c.close > upper }
            }
            // Candle pertama yang punya ATR: mulai sebagai tren turun, seperti TradingView.
            _ => false,
        };

        value[i] = Some(if up { lower } else { upper });
        uptrend[i] = Some(up);
        prev_upper = upper;
        prev_lower = lower;
        prev_up = Some(up);
    }
    Supertrend { value, uptrend }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indicator::test_util::candle;

    #[test]
    fn berbalik_naik_saat_harga_menembus_pita_atas() {
        let mut candles: Vec<Candle> = (0..8).map(|_| candle(11.0, 9.0, 10.0, 1.0)).collect();
        // lonjakan besar di atas pita atas (10 + 3 × ATR 2 = 16)
        candles.extend((0..4).map(|i| candle(31.0 + f64::from(i), 29.0 + f64::from(i), 30.0 + f64::from(i), 1.0)));
        let out = supertrend(&candles, 3, 3.0);
        assert_eq!(out.uptrend[1], None);
        assert_eq!(out.uptrend[2], Some(false));
        assert_eq!(out.uptrend[7], Some(false));
        assert_eq!(out.uptrend[8], Some(true));
        // saat tren naik, garis berada di bawah harga (jadi support)
        let last = candles.len() - 1;
        assert!(out.value[last].unwrap() < candles[last].close);
    }
}
