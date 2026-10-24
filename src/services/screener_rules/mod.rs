//! Tabel `user_screener_rules` — indikator + kondisi di dalam strategi screening. Tidak punya
//! endpoint sendiri: dikelola lewat endpoint `screeners` (PUT strategi mengganti semua rule-nya).

pub mod model;
pub mod repository;
pub mod service;
pub mod types;
