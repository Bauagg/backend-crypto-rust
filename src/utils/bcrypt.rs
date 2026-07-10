use crate::utils::app_error::AppError;

const COST: u32 = 10;

pub fn hash_password(password: &str) -> Result<String, AppError> {
    bcrypt::hash(password, COST)
        .map_err(|_| AppError::Internal("Gagal memproses password".to_string()))
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    bcrypt::verify(password, hash).unwrap_or(false)
}
