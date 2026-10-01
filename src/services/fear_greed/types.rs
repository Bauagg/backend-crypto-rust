use serde::Serialize;

/// Satu titik data Fear & Greed Index — siap ditampilkan FE.
#[derive(Debug, Serialize)]
pub struct FngPoint {
    pub date: String,
    pub fng_value: i16,
}
