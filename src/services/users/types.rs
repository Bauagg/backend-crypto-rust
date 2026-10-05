use rust_decimal::Decimal;
use serde::Deserialize;
use validator::Validate;

#[derive(Debug, Deserialize, Validate)]
pub struct RegisterInput {
    #[validate(length(min = 2, max = 100, message = "Nama lengkap wajib diisi, 2-100 karakter"))]
    pub full_name: String,

    #[validate(email(message = "Format email tidak valid"))]
    pub email: String,

    #[validate(length(min = 10, max = 15, message = "Nomor telepon wajib diisi, 10-15 digit"))]
    #[validate(custom(function = "validate_phone_numeric"))]
    pub phone: String,

    #[validate(length(min = 8, message = "Password wajib diisi, minimal 8 karakter"))]
    pub password: String,

    #[serde(default)]
    #[validate(custom(function = "validate_role"))]
    pub role: Option<String>,
}

fn validate_phone_numeric(phone: &str) -> Result<(), validator::ValidationError> {
    if phone.chars().all(|c| c.is_ascii_digit()) {
        Ok(())
    } else {
        Err(validator::ValidationError::new("phone_numeric")
            .with_message("Nomor telepon hanya boleh angka".into()))
    }
}

fn validate_role(role: &str) -> Result<(), validator::ValidationError> {
    if role == "admin" || role == "user" {
        Ok(())
    } else {
        Err(validator::ValidationError::new("invalid_role")
            .with_message("Role harus admin atau user".into()))
    }
}

/// Daftar exchange yang didukung — dicek case-insensitive lewat `is_supported_platform`,
/// tapi disimpan apa adanya sesuai input user (tidak dipaksa lowercase) supaya tampilan
/// di FE tetap sesuai yang diketik user (mis. "Binance").
pub const SUPPORTED_PLATFORMS: [&str; 1] = ["binance"];

pub fn is_supported_platform(platform: &str) -> bool {
    SUPPORTED_PLATFORMS.contains(&platform.to_lowercase().as_str())
}

fn validate_platform(platform: &str) -> Result<(), validator::ValidationError> {
    if is_supported_platform(platform) {
        Ok(())
    } else {
        Err(validator::ValidationError::new("unsupported_platform").with_message(
            format!("Platform harus salah satu dari: {}", SUPPORTED_PLATFORMS.join(", ")).into(),
        ))
    }
}

/// API key/secret Binance selalu 64 karakter alfanumerik (huruf+angka, HMAC key standar mereka).
fn validate_binance_credential(value: &str) -> Result<(), validator::ValidationError> {
    if value.len() == 64 && value.chars().all(|c| c.is_ascii_alphanumeric()) {
        Ok(())
    } else {
        Err(validator::ValidationError::new("invalid_binance_credential")
            .with_message("Harus 64 karakter alfanumerik (format API key/secret Binance)".into()))
    }
}

#[derive(Debug, Deserialize, Validate)]
pub struct LoginInput {
    #[validate(email(message = "Format email tidak valid"))]
    pub email: String,

    #[validate(length(min = 1, message = "Password wajib diisi"))]
    pub password: String,
}

#[derive(Debug, Deserialize, Validate)]
pub struct RefreshTokenInput {
    #[validate(length(min = 1, message = "refresh_token wajib diisi"))]
    pub refresh_token: String,
}

/// Dipakai saat update profil: semua field opsional (isi yang mau diubah saja),
/// termasuk kredensial exchange (platform/api_key/api_secret) dan foto profil (ditangani terpisah dari multipart).
#[derive(Debug, Default, Deserialize, Validate)]
pub struct UpdateProfileInput {
    #[validate(length(min = 2, max = 100, message = "Nama lengkap harus 2-100 karakter"))]
    pub full_name: Option<String>,

    #[validate(email(message = "Format email tidak valid"))]
    pub email: Option<String>,

    #[validate(length(min = 10, max = 15, message = "Nomor telepon harus 10-15 digit"))]
    pub phone: Option<String>,

    #[validate(custom(function = "validate_platform"))]
    pub platform: Option<String>,

    #[validate(custom(function = "validate_binance_credential"))]
    pub api_key: Option<String>,

    #[validate(custom(function = "validate_binance_credential"))]
    pub api_secret: Option<String>,

    /// Saldo akun demo (representasi USD) — user boleh update nominalnya sendiri.
    /// Validasi tidak-boleh-negatif dicek manual di service (crate `validator` tidak
    /// mendukung `range` untuk tipe `Decimal`).
    pub demo_balance: Option<Decimal>,

    /// Nyalakan/matikan robot trading di akun demo.
    pub is_robot_demo_active: Option<bool>,
    /// Nyalakan/matikan robot trading di akun real/platform.
    pub is_robot_platform_active: Option<bool>,

    /// Mata uang pilihan: `IDR` | `USDT` (dicek di service).
    pub preferred_currency: Option<String>,
}
