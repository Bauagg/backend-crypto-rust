use std::sync::OnceLock;

use rand::RngCore;
use rust_decimal::Decimal;
use serde::Serialize;
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;
use validator::Validate;

use super::model::User;
use super::repository::{
    create_user, find_user_by_email, find_user_by_id, find_user_by_phone,
    reset_demo_initial_capital, reset_platform_initial_capital, update_user,
};
use super::types::{is_supported_platform, LoginInput, RegisterInput, UpdateProfileInput};
use crate::clients::binance;
use crate::services::documents::service::{update_file_service, upload_file_service, UploadInput};
use crate::services::documents::types::FileMetaInput;
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
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| AppError::Internal("Gagal memulai transaksi".to_string()))?;

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
    tx.commit()
        .await
        .map_err(|_| AppError::Internal("Gagal menyimpan perubahan".to_string()))?;

    let _ = SYSTEM_USER_ID.set(id);
    Ok(id)
}

pub async fn register_service(
    tx: &mut Transaction<'_, Postgres>,
    input: RegisterInput,
) -> Result<AuthResult, AppError> {
    input.validate()?;

    if find_user_by_email(tx, &input.email).await?.is_some() {
        return Err(AppError::Conflict("Email sudah terdaftar".to_string()));
    }
    if find_user_by_phone(tx, &input.phone).await?.is_some() {
        return Err(AppError::Conflict("Nomor telepon sudah terdaftar".to_string()));
    }

    let role = input.role.clone().unwrap_or_else(|| "user".to_string());
    let hashed_password = hash_password(&input.password).await?;

    let user = create_user(
        tx,
        &input.full_name,
        &input.email,
        &input.phone,
        &hashed_password,
        &role,
    )
    .await?;

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

pub async fn login_service(
    tx: &mut Transaction<'_, Postgres>,
    input: LoginInput,
) -> Result<AuthResult, AppError> {
    input.validate()?;

    let user = find_user_by_email(tx, &input.email)
        .await?
        .ok_or_else(|| AppError::Unauthorized("Email atau password salah".to_string()))?;

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

pub async fn get_profile_service(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
) -> Result<UserProfile, AppError> {
    let user = find_user_by_id(tx, user_id)
        .await?
        .ok_or_else(|| AppError::NotFound("User tidak ditemukan".to_string()))?;
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

    if let Some(balance) = input.demo_balance {
        if balance < Decimal::from(1) || balance > Decimal::from(100000) {
            return Err(AppError::BadRequest(
                "Saldo demo harus antara 1 dan 100000".to_string(),
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
                    update_file_service(tx, file_id, Some(upload), FileMetaInput::default(), &email)
                        .await?
                }
                None => {
                    let meta = FileMetaInput {
                        ref_id: Some(user_id),
                        ref_type: Some("USER_PROFILE".to_string()),
                    };
                    upload_file_service(tx, upload, user_id, meta, &email).await?
                }
            };

            (Some(file.id.to_string()), Some(file.file_url))
        }
        None => (existing.photo_id, existing.photo_url),
    };

    // Robot diaktifkan ulang -> mulai hitungan modal awal (kill switch) yang baru.
    if is_robot_demo_active && !existing.is_robot_demo_active {
        reset_demo_initial_capital(tx, user_id).await?;
    }
    if is_robot_platform_active && !existing.is_robot_platform_active {
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
    commit(tx_owned).await?;

    Ok(user.into())
}

async fn begin(pool: &PgPool) -> Result<Transaction<'_, Postgres>, AppError> {
    pool.begin()
        .await
        .map_err(|_| AppError::Internal("Gagal memulai transaksi".to_string()))
}

async fn commit(tx: Transaction<'_, Postgres>) -> Result<(), AppError> {
    tx.commit()
        .await
        .map_err(|_| AppError::Internal("Gagal menyimpan perubahan".to_string()))
}
