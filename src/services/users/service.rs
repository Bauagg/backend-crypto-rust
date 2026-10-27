use std::sync::OnceLock;

use chrono::{Duration, Utc};
use rand::{Rng, RngCore};
use rust_decimal::Decimal;
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;
use validator::Validate;

use crate::template::OtpEmail;
use super::model::User;
use super::repository::{
    close_password_resets, create_password_reset, create_user, find_latest_password_reset,
    find_password_reset_by_token, find_user_by_email, find_user_by_id, find_user_by_phone,
    increment_password_reset_attempts, mark_password_reset_verified, reset_demo_initial_capital,
    reset_platform_initial_capital, update_robot_capital_percent, update_user, update_user_password,
};
use super::types::{
    is_supported_platform, ForgotPasswordInput, LoginInput, RegisterInput, ResetPasswordInput,
    UpdateProfileInput, VerifyOtpInput,
};
use crate::utils::mailer::send_email;
use crate::clients::binance;
use crate::services::documents::service::{update_file_in_tx, upload_file_in_tx, UploadInput};
use crate::services::documents::types::FileMetaInput;
use crate::database::{begin, commit};
use crate::utils::app_error::AppError;
use crate::utils::bcrypt::{hash_password, verify_password};
use crate::utils::crypto::{decrypt, encrypt};
use crate::utils::jwt::{generate_token_pair, JwtPayload, TokenPair};

/// Bytes foto profil baru yang diupload lewat multipart. Disimpan sebagai record di tabel `files`
/// (ref_type="USER_PROFILE") lewat `files::service`, bukan ditulis ke disk langsung oleh modul ini.
pub struct ProfilePhotoUpload {
    pub original_name: String,
    pub mime_type: String,
    pub bytes: Vec<u8>,
}

/// Mata uang yang boleh dipilih user (`preferred_currency`).
const SUPPORTED_CURRENCIES: [&str; 2] = ["IDR", "USDT"];

#[derive(Serialize)]
pub struct UserProfile {
    pub id: Uuid,
    pub full_name: String,
    pub email: String,
    pub phone: String,
    pub role: String,
    pub status: String,
    pub platform: Option<String>,
    pub api_key: Option<String>,
    pub photo_id: Option<String>,
    pub photo_url: Option<String>,
    pub demo_balance: Decimal,
    pub is_robot_demo_active: bool,
    pub is_robot_platform_active: bool,
    pub preferred_currency: String,
    pub demo_initial_capital: Option<Decimal>,
    pub demo_robot_last_run_date: Option<chrono::NaiveDate>,
    pub platform_initial_capital: Option<Decimal>,
    pub platform_robot_last_run_date: Option<chrono::NaiveDate>,
    /// Batas persen saldo yang boleh dipakai robot demo / live (1–100, default 100).
    pub demo_robot_capital_percent: Decimal,
    pub platform_robot_capital_percent: Decimal,
    /// `true` = robot menunggu recovery setelah user menjual manual coin milik robot.
    pub demo_robot_awaiting_recovery: bool,
    pub platform_robot_awaiting_recovery: bool,
}

impl From<User> for UserProfile {
    fn from(user: User) -> Self {
        Self {
            id: user.id,
            full_name: user.full_name,
            email: user.email,
            phone: user.phone,
            role: user.role,
            status: user.status,
            platform: user.platform,
            api_key: user.api_key.as_deref().map(masked_api_key),
            photo_id: user.photo_id,
            photo_url: user.photo_url,
            demo_balance: user.demo_balance,
            is_robot_demo_active: user.is_robot_demo_active,
            is_robot_platform_active: user.is_robot_platform_active,
            preferred_currency: user.preferred_currency,
            demo_initial_capital: user.demo_initial_capital,
            demo_robot_last_run_date: user.demo_robot_last_run_date,
            platform_initial_capital: user.platform_initial_capital,
            platform_robot_last_run_date: user.platform_robot_last_run_date,
            demo_robot_capital_percent: user.demo_robot_capital_percent,
            platform_robot_capital_percent: user.platform_robot_capital_percent,
            demo_robot_awaiting_recovery: user.demo_robot_awaiting_recovery,
            platform_robot_awaiting_recovery: user.platform_robot_awaiting_recovery,
        }
    }
}

/// `api_key` (tersimpan terenkripsi) ditampilkan tersamar, mis. `vmPU****A8x2` — cukup untuk user
/// mengenali key mana yang tersambung tanpa membuka isinya.
fn masked_api_key(encrypted: &str) -> String {
    match decrypt(encrypted) {
        Ok(key) if key.is_ascii() && key.len() > 8 => {
            format!("{}****{}", &key[..4], &key[key.len() - 4..])
        }
        _ => "****".to_string(),
    }
}

#[derive(Serialize)]
pub struct AuthResult {
    #[serde(flatten)]
    pub tokens: TokenPair,
    pub user: UserProfile,
}

/// Akun "System": pemilik data yang dibuat otomatis oleh server (mis. coin baru hasil sync).
/// Dikenali dari emailnya — dibuat sekali saat server pertama kali start, start berikutnya dipakai
/// ulang. Password-nya acak & tidak disimpan di mana pun, jadi akun ini tidak bisa dipakai login.
const SYSTEM_FULL_NAME: &str = "System";
const SYSTEM_EMAIL: &str = "system@system.local";
const SYSTEM_PHONE: &str = "0000000000";

static SYSTEM_USER_ID: OnceLock<Uuid> = OnceLock::new();

/// Id akun System — `None` kalau `ensure_system_user` belum berhasil dijalankan.
pub fn system_user_id() -> Option<Uuid> {
    SYSTEM_USER_ID.get().copied()
}

/// Pastikan akun System ada (buat kalau belum), lalu simpan id-nya untuk `system_user_id()`.
/// Dipanggil sekali saat server start, sebelum worker jalan.
pub async fn ensure_system_user(pool: &PgPool) -> Result<Uuid, AppError> {
    let mut tx = begin(pool).await?;
    let id = match find_user_by_email(&mut tx, SYSTEM_EMAIL).await? {
        Some(user) => user.id,
        None => {
            let mut random = [0u8; 32];
            rand::thread_rng().fill_bytes(&mut random);
            let hashed_password = hash_password(&hex::encode(random)).await?;
            let user = create_user(
                &mut tx,
                SYSTEM_FULL_NAME,
                SYSTEM_EMAIL,
                SYSTEM_PHONE,
                &hashed_password,
                "admin",
            )
            .await?;
            tracing::info!("Akun System dibuat: {}", user.id);
            user.id
        }
    };
    commit(tx).await?;

    let _ = SYSTEM_USER_ID.set(id);
    Ok(id)
}

pub async fn register_service(pool: &PgPool, input: RegisterInput) -> Result<AuthResult, AppError> {
    input.validate()?;

    let role = input.role.clone().unwrap_or_else(|| "user".to_string());
    // Hash bcrypt (lambat, ±ratusan ms) sebelum transaksi dibuka — koneksi DB tidak ikut menunggu.
    let hashed_password = hash_password(&input.password).await?;

    let mut tx = begin(pool).await?;
    if find_user_by_email(&mut tx, &input.email).await?.is_some() {
        return Err(AppError::Conflict("Email sudah terdaftar".to_string()));
    }
    if find_user_by_phone(&mut tx, &input.phone).await?.is_some() {
        return Err(AppError::Conflict("Nomor telepon sudah terdaftar".to_string()));
    }

    let user = create_user(
        &mut tx,
        &input.full_name,
        &input.email,
        &input.phone,
        &hashed_password,
        &role,
    )
    .await?;
    commit(tx).await?;

    let tokens = generate_token_pair(&JwtPayload {
        user_id: user.id.to_string(),
        email: user.email.clone(),
        role: user.role.clone(),
        username: user.full_name.clone(),
    })?;

    Ok(AuthResult {
        tokens,
        user: user.into(),
    })
}

pub async fn login_service(pool: &PgPool, input: LoginInput) -> Result<AuthResult, AppError> {
    input.validate()?;

    let mut tx = begin(pool).await?;
    let user = find_user_by_email(&mut tx, &input.email)
        .await?
        .ok_or_else(|| AppError::Unauthorized("Email atau password salah".to_string()))?;
    commit(tx).await?;

    if !verify_password(&input.password, &user.password).await {
        return Err(AppError::Unauthorized("Email atau password salah".to_string()));
    }

    let tokens = generate_token_pair(&JwtPayload {
        user_id: user.id.to_string(),
        email: user.email.clone(),
        role: user.role.clone(),
        username: user.full_name.clone(),
    })?;

    Ok(AuthResult {
        tokens,
        user: user.into(),
    })
}

pub async fn get_profile_service(pool: &PgPool, user_id: Uuid) -> Result<UserProfile, AppError> {
    let mut tx = begin(pool).await?;
    let user = find_user_by_id(&mut tx, user_id)
        .await?
        .ok_or_else(|| AppError::NotFound("User tidak ditemukan".to_string()))?;
    commit(tx).await?;
    Ok(user.into())
}

/// Update profil user dalam satu request: data diri (nama/email/phone), foto profil,
/// dan kredensial exchange (platform/api_key/api_secret) — semua field opsional, isi yang mau diubah saja.
/// `api_key` & `api_secret` dienkripsi (AES-256-GCM) sebelum disimpan, tidak pernah disimpan
/// plaintext; response hanya memuat `api_key` tersamar.
///
/// Verifikasi kredensial ke Binance (bisa beberapa detik) dijalankan SEBELUM transaksi DB dibuka,
/// supaya koneksi DB tidak tertahan selama menunggu exchange. Transaksi hanya membungkus
/// penulisan (foto + data user).
pub async fn update_profile_service(
    pool: &PgPool,
    user_id: Uuid,
    input: UpdateProfileInput,
    photo: Option<ProfilePhotoUpload>,
) -> Result<UserProfile, AppError> {
    input.validate()?;

    if let Some(balance) = input.demo_balance
        && (balance < Decimal::from(1) || balance > Decimal::from(100000)) {
            return Err(AppError::BadRequest(
                "Saldo demo harus antara 1 dan 100000".to_string(),
            ));
        }

    for percent in [input.demo_robot_capital_percent, input.platform_robot_capital_percent].into_iter().flatten() {
        if percent < Decimal::ONE || percent > Decimal::ONE_HUNDRED {
            return Err(AppError::BadRequest(
                "Persen modal robot harus antara 1 dan 100".to_string(),
            ));
        }
    }

    let existing = {
        let mut tx = begin(pool).await?;
        let existing = find_user_by_id(&mut tx, user_id).await?;
        commit(tx).await?;
        existing.ok_or_else(|| AppError::NotFound("User tidak ditemukan".to_string()))?
    };

    let full_name = input.full_name.unwrap_or(existing.full_name);
    let email = input.email.unwrap_or(existing.email);
    let phone = input.phone.unwrap_or(existing.phone);

    let platform = input.platform.clone().or(existing.platform.clone());

    // Kredensial exchange baru wajib diverifikasi ke Binance dulu sebelum disimpan — hanya kalau
    // request ini memang mengirimnya (bukan setiap update profil biasa). api_key & api_secret
    // wajib dikirim berpasangan: keduanya tersimpan terenkripsi, jadi verifikasi selalu memakai
    // pasangan baru dari request, tidak dicampur dengan yang lama.
    let (api_key, api_secret) = match (input.api_key.as_deref(), input.api_secret.as_deref()) {
        (None, None) => (existing.api_key.clone(), existing.api_secret.clone()),
        (Some(key), Some(secret)) => {
            let platform_name = platform.as_deref().ok_or_else(|| {
                AppError::BadRequest("platform wajib diisi untuk verifikasi kredensial".to_string())
            })?;
            if !is_supported_platform(platform_name) {
                return Err(AppError::BadRequest(format!(
                    "Platform '{platform_name}' belum didukung untuk verifikasi kredensial"
                )));
            }
            binance::verify_credentials(key, secret).await?;
            (Some(encrypt(key)?), Some(encrypt(secret)?))
        }
        _ => {
            return Err(AppError::BadRequest(
                "api_key dan api_secret wajib dikirim bersamaan".to_string(),
            ));
        }
    };
    let demo_balance = input.demo_balance.unwrap_or(existing.demo_balance);
    let is_robot_demo_active = input.is_robot_demo_active.unwrap_or(existing.is_robot_demo_active);
    let is_robot_platform_active = input
        .is_robot_platform_active
        .unwrap_or(existing.is_robot_platform_active);
    let preferred_currency = match input.preferred_currency.as_deref().map(str::trim) {
        Some(currency) => {
            let currency = currency.to_uppercase();
            if !SUPPORTED_CURRENCIES.contains(&currency.as_str()) {
                return Err(AppError::BadRequest(
                    "preferred_currency harus IDR atau USDT".to_string(),
                ));
            }
            currency
        }
        None => existing.preferred_currency,
    };

    // Robot platform (akun real) cuma boleh aktif kalau ada kredensial exchange lengkap --
    // platform + api_key + api_secret. Kalau request ini kirim api_key/api_secret baru, itu
    // sudah diverifikasi ke Binance di atas; kalau tidak, berarti mengandalkan kredensial lama
    // yang tersimpan (sudah pernah diverifikasi saat pertama kali disimpan).
    if is_robot_platform_active && (platform.is_none() || api_key.is_none() || api_secret.is_none()) {
        return Err(AppError::BadRequest(
            "Robot platform tidak bisa diaktifkan tanpa platform, api_key, dan api_secret yang valid".to_string(),
        ));
    }

    // Mulai di sini hanya baca/tulis DB — transaksi singkat.
    let mut tx_owned = begin(pool).await?;
    let tx = &mut tx_owned;

    let (photo_id, photo_url) = match photo {
        Some(photo) => {
            let upload = UploadInput {
                original_name: photo.original_name,
                mime_type: photo.mime_type,
                bytes: photo.bytes,
            };

            let existing_photo_id = existing
                .photo_id
                .as_deref()
                .and_then(|id| Uuid::parse_str(id).ok());

            let file = match existing_photo_id {
                Some(file_id) => {
                    update_file_in_tx(tx, file_id, Some(upload), FileMetaInput::default(), &email)
                        .await?
                }
                None => {
                    let meta = FileMetaInput {
                        ref_id: Some(user_id),
                        ref_type: Some("USER_PROFILE".to_string()),
                    };
                    upload_file_in_tx(tx, upload, user_id, meta, &email).await?
                }
            };

            (Some(file.id.to_string()), Some(file.file_url))
        }
        None => (existing.photo_id, existing.photo_url),
    };

    let demo_percent = input
        .demo_robot_capital_percent
        .map(|p| p.round_dp(2))
        .unwrap_or(existing.demo_robot_capital_percent);
    let platform_percent = input
        .platform_robot_capital_percent
        .map(|p| p.round_dp(2))
        .unwrap_or(existing.platform_robot_capital_percent);

    // Robot diaktifkan ulang, atau batas modalnya diubah -> mulai hitungan modal awal (kill switch)
    // yang baru. Tanpa reset, modal yang mengecil karena persen diturunkan terbaca sebagai rugi.
    if (is_robot_demo_active && !existing.is_robot_demo_active)
        || demo_percent != existing.demo_robot_capital_percent
    {
        reset_demo_initial_capital(tx, user_id).await?;
    }
    if (is_robot_platform_active && !existing.is_robot_platform_active)
        || platform_percent != existing.platform_robot_capital_percent
    {
        reset_platform_initial_capital(tx, user_id).await?;
    }

    let user = update_user(
        tx,
        user_id,
        &full_name,
        &email,
        &phone,
        platform.as_deref(),
        api_key.as_deref(),
        api_secret.as_deref(),
        photo_id.as_deref(),
        photo_url.as_deref(),
        demo_balance,
        is_robot_demo_active,
        is_robot_platform_active,
        &preferred_currency,
    )
    .await?;
    let user = if demo_percent != user.demo_robot_capital_percent
        || platform_percent != user.platform_robot_capital_percent
    {
        update_robot_capital_percent(tx, user_id, demo_percent, platform_percent).await?
    } else {
        user
    };
    commit(tx_owned).await?;

    Ok(user.into())
}

// ---------------------------------------------------------------------------------------------
// Lupa password: email -> kode OTP 6 digit -> password baru.
// ---------------------------------------------------------------------------------------------

/// Kode OTP berlaku selama ini sejak dikirim.
const OTP_VALID_MINUTES: i64 = 10;
/// Jeda minimal sebelum boleh minta kode baru (cegah spam email).
const OTP_RESEND_COOLDOWN_SECONDS: i64 = 60;
/// Salah kode lebih dari ini -> harus minta kode baru (kode 6 digit tidak bisa ditebak beruntun).
const OTP_MAX_ATTEMPTS: i32 = 5;
/// Setelah OTP benar, user punya waktu segini untuk mengisi password baru.
const RESET_TOKEN_VALID_MINUTES: i64 = 15;

/// Pesan sama untuk email terdaftar maupun tidak, supaya endpoint ini tidak bisa dipakai menebak
/// email siapa saja yang punya akun.
pub const FORGOT_PASSWORD_MESSAGE: &str =
    "Jika email terdaftar, kode OTP sudah dikirim. Cek inbox atau folder spam.";
const INVALID_OTP_MESSAGE: &str = "Kode OTP salah atau sudah kedaluwarsa";

fn sha256_hex(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))
}

/// Hash OTP diikat ke user, supaya kode yang sama milik user lain menghasilkan hash berbeda.
fn hash_otp(user_id: Uuid, otp: &str) -> String {
    sha256_hex(&format!("{user_id}:{otp}"))
}

fn generate_otp() -> String {
    format!("{:06}", rand::thread_rng().gen_range(0..1_000_000))
}

fn generate_reset_token() -> String {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    hex::encode(bytes)
}

fn normalize_email(email: &str) -> String {
    email.trim().to_string()
}

/// Langkah 1: buat kode OTP & kirim ke email user. Email yang tidak terdaftar dijawab dengan pesan
/// yang sama (tanpa kirim email). Permintaan baru membatalkan kode lama.
pub async fn forgot_password_service(pool: &PgPool, input: ForgotPasswordInput) -> Result<(), AppError> {
    input.validate()?;
    let email = normalize_email(&input.email);

    let mut tx = begin(pool).await?;
    let Some(user) = find_user_by_email(&mut tx, &email).await? else {
        commit(tx).await?;
        return Ok(());
    };
    if user.email == SYSTEM_EMAIL {
        commit(tx).await?;
        return Ok(());
    }

    if let Some(latest) = find_latest_password_reset(&mut tx, user.id).await? {
        let elapsed = (Utc::now() - latest.created_at).num_seconds();
        if elapsed < OTP_RESEND_COOLDOWN_SECONDS {
            return Err(AppError::BadRequest(format!(
                "Tunggu {} detik sebelum meminta kode baru",
                OTP_RESEND_COOLDOWN_SECONDS - elapsed
            )));
        }
    }

    let otp = generate_otp();
    close_password_resets(&mut tx, user.id).await?;
    create_password_reset(
        &mut tx,
        user.id,
        &hash_otp(user.id, &otp),
        Utc::now() + Duration::minutes(OTP_VALID_MINUTES),
    )
    .await?;
    commit(tx).await?;

    // Kirim email di luar transaksi — koneksi DB tidak tertahan selama menunggu server SMTP.
    let email = OtpEmail {
        name: &user.full_name,
        code: &otp,
        valid_minutes: OTP_VALID_MINUTES,
        label: "Reset password",
        subject: "kode reset password Anda",
        message: "Kami menerima permintaan untuk mengatur ulang password akun Anda. \
                  Masukkan kode berikut di aplikasi untuk melanjutkan.",
        ignore_note: "Tidak merasa meminta reset password? Abaikan email ini — password Anda \
                      tidak berubah dan akun tetap aman.",
    }
    .render();
    send_email(&user.email, &email.subject, email.text, email.html).await
}

#[derive(Serialize)]
pub struct VerifyOtpResult {
    /// Dipakai di langkah 3 (`/reset-password`). Rahasia — jangan disimpan permanen di FE.
    pub reset_token: String,
    pub expires_in_minutes: i64,
}

/// Langkah 2: cocokkan kode OTP. Benar -> dapat `reset_token` sekali pakai untuk langkah 3.
pub async fn verify_otp_service(pool: &PgPool, input: VerifyOtpInput) -> Result<VerifyOtpResult, AppError> {
    input.validate()?;
    let email = normalize_email(&input.email);
    let invalid = || AppError::BadRequest(INVALID_OTP_MESSAGE.to_string());

    let mut tx = begin(pool).await?;
    let user = find_user_by_email(&mut tx, &email).await?.ok_or_else(invalid)?;
    let reset = find_latest_password_reset(&mut tx, user.id)
        .await?
        .filter(|r| r.verified_at.is_none() && r.otp_expires_at > Utc::now())
        .ok_or_else(invalid)?;

    if reset.attempts >= OTP_MAX_ATTEMPTS {
        return Err(AppError::BadRequest(
            "Terlalu banyak percobaan. Minta kode OTP baru".to_string(),
        ));
    }

    if reset.otp_hash != hash_otp(user.id, input.otp.trim()) {
        increment_password_reset_attempts(&mut tx, reset.id).await?;
        commit(tx).await?;
        let remaining = OTP_MAX_ATTEMPTS - reset.attempts - 1;
        return Err(AppError::BadRequest(if remaining > 0 {
            format!("Kode OTP salah. Sisa {remaining} percobaan")
        } else {
            "Kode OTP salah. Minta kode OTP baru".to_string()
        }));
    }

    let reset_token = generate_reset_token();
    mark_password_reset_verified(
        &mut tx,
        reset.id,
        &sha256_hex(&reset_token),
        Utc::now() + Duration::minutes(RESET_TOKEN_VALID_MINUTES),
    )
    .await?;
    commit(tx).await?;

    Ok(VerifyOtpResult {
        reset_token,
        expires_in_minutes: RESET_TOKEN_VALID_MINUTES,
    })
}

/// Langkah 3: ganti password memakai `reset_token` dari langkah 2. Token hanya bisa dipakai sekali.
pub async fn reset_password_service(pool: &PgPool, input: ResetPasswordInput) -> Result<(), AppError> {
    input.validate()?;
    if input.new_password != input.confirm_password {
        return Err(AppError::BadRequest(
            "Konfirmasi password tidak sama dengan password baru".to_string(),
        ));
    }

    // Hash password dulu (lambat, bcrypt) sebelum membuka transaksi.
    let hashed_password = hash_password(&input.new_password).await?;

    let mut tx = begin(pool).await?;
    let reset = find_password_reset_by_token(&mut tx, &sha256_hex(input.reset_token.trim()))
        .await?
        .ok_or_else(|| {
            AppError::BadRequest(
                "Sesi reset password tidak valid atau sudah kedaluwarsa. Ulangi dari awal".to_string(),
            )
        })?;

    update_user_password(&mut tx, reset.user_id, &hashed_password).await?;
    close_password_resets(&mut tx, reset.user_id).await?;
    commit(tx).await?;
    Ok(())
}

#[cfg(test)]
mod password_reset_tests {
    use super::*;

    #[test]
    fn otp_selalu_6_digit_angka() {
        for _ in 0..200 {
            let otp = generate_otp();
            assert_eq!(otp.len(), 6);
            assert!(otp.chars().all(|c| c.is_ascii_digit()));
        }
    }

    #[test]
    fn hash_otp_terikat_ke_user() {
        let (a, b) = (Uuid::new_v4(), Uuid::new_v4());
        assert_eq!(hash_otp(a, "123456"), hash_otp(a, "123456"));
        assert_ne!(hash_otp(a, "123456"), hash_otp(b, "123456"));
        assert_ne!(hash_otp(a, "123456"), hash_otp(a, "123457"));
    }

    #[test]
    fn reset_token_acak_64_hex() {
        let (a, b) = (generate_reset_token(), generate_reset_token());
        assert_eq!(a.len(), 64);
        assert_ne!(a, b);
    }
}
