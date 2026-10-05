use std::sync::OnceLock;

use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::{engine::general_purpose::STANDARD, Engine};
use rand::RngCore;

use crate::utils::app_error::AppError;

const NONCE_LEN: usize = 12;

static CIPHER: OnceLock<Aes256Gcm> = OnceLock::new();

/// Key diambil dari env `ENCRYPTION_KEY`, harus persis 32 byte (bisa berupa string biasa,
/// dipad/dipotong ke 32 byte). Dipakai untuk enkripsi/dekripsi `api_key` & `api_secret` exchange.
/// Dibaca sekali saja, lalu dipakai ulang.
fn cipher() -> Result<&'static Aes256Gcm, AppError> {
    if let Some(cipher) = CIPHER.get() {
        return Ok(cipher);
    }
    let raw_key = std::env::var("ENCRYPTION_KEY")
        .ok()
        .filter(|v| !v.is_empty())
        .ok_or_else(|| AppError::Internal("ENCRYPTION_KEY tidak ditemukan di .env".to_string()))?;

    let mut key_bytes = [0u8; 32];
    let raw = raw_key.as_bytes();
    let len = raw.len().min(32);
    key_bytes[..len].copy_from_slice(&raw[..len]);

    let key = Key::<Aes256Gcm>::from_slice(&key_bytes);
    Ok(CIPHER.get_or_init(|| Aes256Gcm::new(key)))
}

/// Baca kunci enkripsi saat server start — env kosong = server gagal start, bukan error saat
/// user menyimpan/memakai API key.
pub fn init() -> Result<(), AppError> {
    cipher().map(|_| ())
}

/// Enkripsi plaintext (mis. api_secret exchange) -> base64(nonce || ciphertext).
pub fn encrypt(plaintext: &str) -> Result<String, AppError> {
    let cipher = cipher()?;

    let mut nonce_bytes = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|_| AppError::Internal("Gagal mengenkripsi data".to_string()))?;

    let mut combined = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    combined.extend_from_slice(&nonce_bytes);
    combined.extend_from_slice(&ciphertext);

    Ok(STANDARD.encode(combined))
}

/// Dekripsi hasil dari `encrypt`, kembalikan plaintext original.
/// Dipakai saat request ke Binance (key & secret user) dan untuk menyamarkan `api_key` di profil.
pub fn decrypt(encoded: &str) -> Result<String, AppError> {
    let cipher = cipher()?;

    let combined = STANDARD
        .decode(encoded)
        .map_err(|_| AppError::Internal("Gagal mendekode data terenkripsi".to_string()))?;

    if combined.len() < NONCE_LEN {
        return Err(AppError::Internal("Data terenkripsi tidak valid".to_string()));
    }

    let (nonce_bytes, ciphertext) = combined.split_at(NONCE_LEN);
    let nonce = Nonce::from_slice(nonce_bytes);

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| AppError::Internal("Gagal mendekripsi data".to_string()))?;

    String::from_utf8(plaintext)
        .map_err(|_| AppError::Internal("Data hasil dekripsi tidak valid".to_string()))
}
