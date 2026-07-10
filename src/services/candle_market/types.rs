use serde::Deserialize;

/// Query untuk GET /api/candle-market — histori candle tersimpan (hanya interval 1d, dikumpulkan
/// sendiri lewat `collector`/`backfill`). Berbeda dari `market::types::KlinesQuery` yang punya
/// 15 interval karena itu proxy live ke Tokocrypto; di sini cuma 1d yang benar-benar ada datanya.
#[derive(Debug, Deserialize)]
pub struct StoredCandlesQuery {
    pub symbol: String,
    #[serde(default)]
    pub limit: Option<u16>,
    /// Untuk pagination geser-ke-kiri: isi dengan `open_time` candle paling lama yang sudah
    /// dimiliki FE, dapat balik candle-candle SEBELUM waktu itu (histori lebih lama).
    #[serde(default)]
    pub end_time: Option<i64>,
}
