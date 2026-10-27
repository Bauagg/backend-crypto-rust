//! Tabel `strategy_versions` — isi script strategi per versi (immutable). Tidak punya endpoint
//! sendiri: versi dibuat & dibaca lewat endpoint `strategies`.

pub mod model;
pub mod repository;
pub mod service;
pub mod types;
