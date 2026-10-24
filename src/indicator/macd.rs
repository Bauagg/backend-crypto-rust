//! MACD — Moving Average Convergence Divergence (`ta.macd`): arah & kekuatan tren.
//! Garis MACD = EMA cepat − EMA lambat; garis sinyal = EMA dari garis MACD; histogram = selisihnya.
//! MACD memotong ke atas sinyal = tanda naik (bullish cross), ke bawah = tanda turun.

use super::ema::ema;
use super::on_defined;

pub const DEFAULT_FAST: usize = 12;
pub const DEFAULT_SLOW: usize = 26;
pub const DEFAULT_SIGNAL: usize = 9;

#[derive(Debug, Clone, PartialEq)]
pub struct Macd {
    pub macd: Vec<Option<f64>>,
    pub signal: Vec<Option<f64>>,
    pub histogram: Vec<Option<f64>>,
}

pub fn macd(closes: &[f64], fast: usize, slow: usize, signal: usize) -> Macd {
    let fast_ema = ema(closes, fast);
    let slow_ema = ema(closes, slow);
    let line: Vec<Option<f64>> = fast_ema
        .iter()
        .zip(&slow_ema)
        .map(|(f, s)| Some((*f)? - (*s)?))
        .collect();
    let signal_line = on_defined(&line, |values| ema(values, signal));
    let histogram = line
        .iter()
        .zip(&signal_line)
        .map(|(m, s)| Some((*m)? - (*s)?))
        .collect();
    Macd { macd: line, signal: signal_line, histogram }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indicator::test_util::approx;

    #[test]
    fn garis_macd_dan_sinyal() {
        let closes: Vec<f64> = (1..=40).map(f64::from).collect();
        let out = macd(&closes, 3, 6, 3);
        // garis MACD mulai saat EMA lambat (6) terisi, sinyal 3 data sesudahnya
        assert_eq!(out.macd[4], None);
        assert!(out.macd[5].is_some());
        assert_eq!(out.signal[6], None);
        assert!(out.signal[7].is_some());
        // harga naik linear -> EMA 3 & 6 tertinggal tetap: (6−1)/2 − (3−1)/2 = 1.5
        approx(*out.macd.last().unwrap(), 1.5);
        approx(*out.histogram.last().unwrap(), 0.0);
    }
}
