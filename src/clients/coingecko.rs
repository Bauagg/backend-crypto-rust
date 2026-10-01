//! Klien CoinGecko — data market cap & logo coin.

use serde::Deserialize;

use crate::utils::app_error::AppError;
use crate::utils::http_client::get_json_with_headers;

/// CoinGecko menolak (403) request tanpa User-Agent.
const USER_AGENT: &str = "backend-treding-rust";

fn api_base_url() -> String {
    std::env::var("COINGECKO_API_URL")
        .unwrap_or_else(|_| "https://api.coingecko.com/api/v3".to_string())
}

/// 1 item `/coins/markets`.
#[derive(Debug, Deserialize)]
pub struct CoinMarket {
    pub symbol: String,
    pub name: String,
    pub market_cap: Option<f64>,
    /// URL logo resmi coin.
    pub image: Option<String>,
}

/// `per_page` coin teratas berdasarkan market cap (USD), terbesar dulu. Maksimal 250 per halaman.
pub async fn get_top_markets(per_page: u16) -> Result<Vec<CoinMarket>, AppError> {
    let url = format!(
        "{}/coins/markets?vs_currency=usd&order=market_cap_desc&per_page={per_page}&page=1",
        api_base_url().trim_end_matches('/')
    );
    get_json_with_headers(&url, vec![("user-agent", USER_AGENT.to_string())]).await
}
