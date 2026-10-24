//! ADX — Average Directional Index (`ta.dmi`): KEKUATAN tren (bukan arahnya), 0–100.
//! ADX di atas 25 = tren kuat, di bawah 20 = pasar datar. Arahnya dari +DI vs −DI: +DI di atas
//! −DI = tren naik.

use super::atr::true_range;
use super::{rma, Candle};

pub const DEFAULT_PERIOD: usize = 14;
pub const STRONG_TREND: f64 = 25.0;

#[derive(Debug, Clone, PartialEq)]
pub struct Adx {
    pub adx: Vec<Option<f64>>,
    pub plus_di: Vec<Option<f64>>,
    pub minus_di: Vec<Option<f64>>,
}

pub fn adx(candles: &[Candle], period: usize) -> Adx {
    let len = candles.len();
    let (mut plus_dm, mut minus_dm) = (vec![0.0; len], vec![0.0; len]);
    for i in 1..len {
        let up = candles[i].high - candles[i - 1].high;
        let down = candles[i - 1].low - candles[i].low;
        if up > down && up > 0.0 {
            plus_dm[i] = up;
        }
        if down > up && down > 0.0 {
            minus_dm[i] = down;
        }
    }
    // Candle pertama tidak punya pembanding — rata-rata dimulai dari candle kedua (seperti TradingView).
    let skip_first = |values: &[f64]| -> Vec<Option<f64>> {
        let mut out = vec![None];
        out.extend(rma(values.get(1..).unwrap_or_default(), period));
        out.truncate(len);
        out
    };
    let tr = skip_first(&true_range(candles));
    let plus = skip_first(&plus_dm);
    let minus = skip_first(&minus_dm);

    let di = |dm: &[Option<f64>]| -> Vec<Option<f64>> {
        dm.iter()
            .zip(&tr)
            .map(|(dm, tr)| {
                let tr = (*tr)?;
                Some(if tr == 0.0 { 0.0 } else { 100.0 * (*dm)? / tr })
            })
            .collect()
    };
    let plus_di = di(&plus);
    let minus_di = di(&minus);

    let dx: Vec<Option<f64>> = plus_di
        .iter()
        .zip(&minus_di)
        .map(|(p, m)| {
            let (p, m) = ((*p)?, (*m)?);
            Some(if p + m == 0.0 { 0.0 } else { 100.0 * (p - m).abs() / (p + m) })
        })
        .collect();
    let adx = super::on_defined(&dx, |values| rma(values, period));
    Adx { adx, plus_di, minus_di }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indicator::test_util::{approx, candle};

    #[test]
    fn tren_naik_konsisten_adx_tinggi() {
        let candles: Vec<Candle> =
            (0..40).map(|i| candle(f64::from(i) + 1.0, f64::from(i), f64::from(i) + 0.5, 1.0)).collect();
        let out = adx(&candles, 14);
        // naik terus: −DM selalu 0 -> −DI 0, DX 100 -> ADX 100
        approx(*out.minus_di.last().unwrap(), 0.0);
        approx(*out.adx.last().unwrap(), 100.0);
        assert!(out.plus_di.last().unwrap().unwrap() > 0.0);
        // warm-up: DI mulai index 14, ADX 14 nilai DX sesudahnya
        assert_eq!(out.plus_di[13], None);
        assert!(out.plus_di[14].is_some());
        assert_eq!(out.adx[26], None);
        assert!(out.adx[27].is_some());
    }
}
