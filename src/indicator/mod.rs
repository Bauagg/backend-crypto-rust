//! Rumus indikator teknikal — 1 file = 1 indikator, nama file = `value_param` di `flex_params`
//! (`type_param = INDICATORS`), mis. `RSI` -> `rsi.rs`, `BOLLINGER_BANDS` -> `bollinger_bands.rs`.
//!
//! Aturan folder ini:
//! - Hitungan murni: input data candle/angka, output angka. Tidak ada DB, API, atau I/O — yang
//!   mengambil candle dari DB & memilih indikator adalah pemanggilnya (mis. screener).
//! - Output deret sepanjang input (`Vec<Option<f64>>`): `None` selama data belum cukup (warm-up),
//!   jadi index ke-i selalu milik candle ke-i.
//! - Rumus mengikuti TradingView (yang biasa dipakai user mencocokkan angka): rata-rata Wilder
//!   (RSI, ATR) dan EMA diawali SMA dari `period` data pertama.
//!
//! Menambah indikator baru (mis. ZigZag): buat `zigzag.rs` di sini + test-nya, daftarkan `mod`
//! di bawah, lalu tambahkan varian di `IndicatorKind` (kode = `value_param` di flex_params).

pub mod adx;
pub mod atr;
pub mod bollinger_bands;
pub mod ema;
pub mod fear_greed;
pub mod ichimoku;
pub mod macd;
pub mod obv;
pub mod rsi;
pub mod sma;
pub mod stoch_rsi;
pub mod stochastic;
pub mod supertrend;
pub mod vwap;

/// 1 candle untuk hitungan indikator. Pemanggil mengonversi dari `candle_ohlcv` (Decimal) ke f64.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Candle {
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

/// Daftar indikator yang punya rumus di folder ini. `code()` = `value_param` di flex_params.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IndicatorKind {
    Rsi,
    Macd,
    Sma,
    Ema,
    BollingerBands,
    Stochastic,
    StochRsi,
    Atr,
    Adx,
    Vwap,
    Obv,
    Ichimoku,
    Supertrend,
    FearGreed,
}

impl IndicatorKind {
    pub const ALL: [IndicatorKind; 14] = [
        IndicatorKind::Rsi,
        IndicatorKind::Macd,
        IndicatorKind::Sma,
        IndicatorKind::Ema,
        IndicatorKind::BollingerBands,
        IndicatorKind::Stochastic,
        IndicatorKind::StochRsi,
        IndicatorKind::Atr,
        IndicatorKind::Adx,
        IndicatorKind::Vwap,
        IndicatorKind::Obv,
        IndicatorKind::Ichimoku,
        IndicatorKind::Supertrend,
        IndicatorKind::FearGreed,
    ];

    pub fn code(self) -> &'static str {
        match self {
            IndicatorKind::Rsi => "RSI",
            IndicatorKind::Macd => "MACD",
            IndicatorKind::Sma => "SMA",
            IndicatorKind::Ema => "EMA",
            IndicatorKind::BollingerBands => "BOLLINGER_BANDS",
            IndicatorKind::Stochastic => "STOCHASTIC",
            IndicatorKind::StochRsi => "STOCH_RSI",
            IndicatorKind::Atr => "ATR",
            IndicatorKind::Adx => "ADX",
            IndicatorKind::Vwap => "VWAP",
            IndicatorKind::Obv => "OBV",
            IndicatorKind::Ichimoku => "ICHIMOKU",
            IndicatorKind::Supertrend => "SUPERTREND",
            IndicatorKind::FearGreed => "FEAR_GREED",
        }
    }

    /// `value_param` flex_params -> indikator (tidak peduli huruf besar/kecil & spasi).
    pub fn from_code(code: &str) -> Option<Self> {
        let code = code.trim();
        Self::ALL.into_iter().find(|kind| kind.code().eq_ignore_ascii_case(code))
    }
}

pub(crate) fn closes(candles: &[Candle]) -> Vec<f64> {
    candles.iter().map(|c| c.close).collect()
}

/// Rata-rata Wilder (RMA, `ta.rma` TradingView): diawali SMA `period` nilai pertama, lalu
/// `rma = (rma_sebelumnya × (period − 1) + nilai) / period`. Dipakai RSI & ATR.
pub(crate) fn rma(values: &[f64], period: usize) -> Vec<Option<f64>> {
    let mut out = vec![None; values.len()];
    if period == 0 || values.len() < period {
        return out;
    }
    let mut avg = values[..period].iter().sum::<f64>() / period as f64;
    out[period - 1] = Some(avg);
    for i in period..values.len() {
        avg = (avg * (period as f64 - 1.0) + values[i]) / period as f64;
        out[i] = Some(avg);
    }
    out
}

/// Terapkan rumus deret ke bagian yang sudah terisi (`Some`) saja, hasil disejajarkan lagi ke
/// index aslinya — mis. EMA dari garis MACD yang baru ada setelah warm-up EMA lambat.
pub(crate) fn on_defined(
    values: &[Option<f64>],
    f: impl Fn(&[f64]) -> Vec<Option<f64>>,
) -> Vec<Option<f64>> {
    let start = values.iter().position(Option::is_some).unwrap_or(values.len());
    let defined: Vec<f64> = values[start..].iter().map(|v| v.unwrap_or(f64::NAN)).collect();
    let mut out = vec![None; start];
    out.extend(f(&defined));
    out
}

#[cfg(test)]
pub(crate) mod test_util {
    use super::Candle;

    pub fn approx(actual: Option<f64>, expected: f64) {
        let actual = actual.expect("nilai belum terisi");
        assert!((actual - expected).abs() < 1e-6, "{actual} != {expected}");
    }

    pub fn candle(high: f64, low: f64, close: f64, volume: f64) -> Candle {
        Candle { open: close, high, low, close, volume }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kode_sama_dengan_flex_params() {
        assert_eq!(IndicatorKind::from_code("rsi"), Some(IndicatorKind::Rsi));
        assert_eq!(IndicatorKind::from_code(" BOLLINGER_BANDS "), Some(IndicatorKind::BollingerBands));
        assert_eq!(IndicatorKind::from_code("ZIGZAG"), None);
        for kind in IndicatorKind::ALL {
            assert_eq!(IndicatorKind::from_code(kind.code()), Some(kind));
        }
    }

    #[test]
    fn rma_diawali_sma() {
        let out = rma(&[1.0, 2.0, 3.0, 4.0], 3);
        assert_eq!(out[1], None);
        test_util::approx(out[2], 2.0);
        test_util::approx(out[3], (2.0 * 2.0 + 4.0) / 3.0);
    }
}
