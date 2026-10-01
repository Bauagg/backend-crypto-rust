use std::time::Duration;

use sqlx::PgPool;

use super::service::sync_coin_symbols_service;

/// Sync cuma menyamakan daftar coin dengan exchange (listing/delisting & volume 24 jam berubah
/// pelan) — sekali sehari sudah cukup.
const SYNC_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);

/// Jalan sebagai background task selama server hidup: sync daftar `SIMBOL_CRYPTO` (termasuk
/// kategori & logo dari CoinGecko) sekali sehari. Sync pertama saat startup dipanggil terpisah
/// lewat `sync_once` (lihat `main.rs`), jadi loop ini mulai dari putaran ke-2.
pub async fn start(pool: PgPool) {
    loop {
        tokio::time::sleep(SYNC_INTERVAL).await;
        sync_once(&pool).await;
    }
}

pub async fn sync_once(pool: &PgPool) {
    let mut tx = match pool.begin().await {
        Ok(tx) => tx,
        Err(err) => {
            tracing::error!("Gagal memulai transaksi untuk sync coin symbols: {err}");
            return;
        }
    };

    match sync_coin_symbols_service(&mut tx).await {
        Ok(summary) => match tx.commit().await {
            Ok(()) => tracing::info!(
                "Sync coin symbols selesai: {} baru, {} dipulihkan, {} di-soft-delete, {} ganti kategori, {} foto baru",
                summary.created,
                summary.restored,
                summary.deleted,
                summary.recategorized,
                summary.photos
            ),
            Err(err) => tracing::error!("Gagal commit sync coin symbols: {err}"),
        },
        Err(err) => {
            let _ = tx.rollback().await;
            tracing::error!("Sync coin symbols gagal: {err:?}");
        }
    }
}
