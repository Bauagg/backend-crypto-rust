/// Data CoinGecko yang dipakai sync per coin.
#[derive(Debug)]
pub struct CoinGeckoCoin {
    pub market_cap: f64,
    pub image: Option<String>,
}

/// Ringkasan hasil 1 kali sync, untuk log.
#[derive(Debug, Default)]
pub struct SyncSummary {
    pub created: usize,
    pub restored: usize,
    pub deleted: usize,
    pub recategorized: usize,
    pub photos: usize,
}
