use crate::utils::app_error::AppError;

const COST: u32 = 10;

/// bcrypt sengaja lambat (±50–100 ms CPU per panggilan) — dijalankan di thread pool blocking
/// supaya tidak menahan thread async yang melayani request lain.
pub async fn hash_password(password: &str) -> Result<String, AppError> {
    let password = password.to_string();
    tokio::task::spawn_blocking(move || bcrypt::hash(password, COST))
        .await
        .map_err(|_| AppError::Internal("Gagal memproses password".to_string()))?
        .map_err(|_| AppError::Internal("Gagal memproses password".to_string()))
}

pub async fn verify_password(password: &str, hash: &str) -> bool {
    let (password, hash) = (password.to_string(), hash.to_string());
    tokio::task::spawn_blocking(move || bcrypt::verify(password, &hash).unwrap_or(false))
        .await
        .unwrap_or(false)
}
