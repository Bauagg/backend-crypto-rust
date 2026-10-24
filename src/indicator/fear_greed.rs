//! Fear & Greed Index — sentimen pasar crypto 0–100 (data harian alternative.me di tabel
//! `fear_greed_index`). Berlaku untuk SELURUH pasar, bukan per coin: dipakai sebagai filter
//! kondisi pasar, mis. hanya cari coin saat pasar tidak sedang serakah.
//! Di bawah 25 = pasar ketakutan (sering jadi peluang beli), di atas 75 = serakah (hati-hati).

use super::sma::sma;

/// Rata-rata 14 hari — sama dengan filter pasar API strategi Python (`FGI_WINDOW`).
pub const DEFAULT_PERIOD: usize = 14;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    ExtremeFear,
    Fear,
    Neutral,
    Greed,
    ExtremeGreed,
}

impl Zone {
    /// Batas zona: 0–24 / 25–44 / 45–55 / 56–75 / 76–100.
    pub fn of(value: f64) -> Self {
        match value {
            v if v < 25.0 => Zone::ExtremeFear,
            v if v < 45.0 => Zone::Fear,
            v if v <= 55.0 => Zone::Neutral,
            v if v <= 75.0 => Zone::Greed,
            _ => Zone::ExtremeGreed,
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Zone::ExtremeFear => "EXTREME_FEAR",
            Zone::Fear => "FEAR",
            Zone::Neutral => "NEUTRAL",
            Zone::Greed => "GREED",
            Zone::ExtremeGreed => "EXTREME_GREED",
        }
    }
}

/// Rata-rata Fear & Greed `period` hari (meredam lonjakan 1 hari).
pub fn average(values: &[f64], period: usize) -> Vec<Option<f64>> {
    sma(values, period)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indicator::test_util::approx;

    #[test]
    fn zona_sentimen() {
        assert_eq!(Zone::of(10.0), Zone::ExtremeFear);
        assert_eq!(Zone::of(25.0), Zone::Fear);
        assert_eq!(Zone::of(50.0), Zone::Neutral);
        assert_eq!(Zone::of(70.0), Zone::Greed);
        assert_eq!(Zone::of(90.0).code(), "EXTREME_GREED");
    }

    #[test]
    fn rata_rata_harian() {
        approx(average(&[20.0, 40.0, 60.0], 3)[2], 40.0);
    }
}
