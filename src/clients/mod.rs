//! Klien API pihak ketiga. Hanya modul di sini yang tahu URL, format request/respons, dan
//! autentikasi masing-masing layanan luar — `services` cukup memanggil fungsinya.

pub mod alternative_me;
pub mod binance;
pub mod coingecko;
pub mod strategy_api;
