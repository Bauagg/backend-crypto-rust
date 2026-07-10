use serde::{Deserialize, Serialize};

/// Bentuk mentah respons https://api.alternative.me/fng/ — dipakai untuk parsing internal saja.
#[derive(Debug, Deserialize)]
pub struct RawFngResponse {
    pub data: Vec<RawFngEntry>,
}

#[derive(Debug, Deserialize)]
pub struct RawFngEntry {
    pub value: String,
    pub timestamp: String,
}

/// Satu titik data Fear & Greed Index — siap ditampilkan FE.
#[derive(Debug, Serialize)]
pub struct FngPoint {
    pub date: String,
    pub fng_value: i16,
}
