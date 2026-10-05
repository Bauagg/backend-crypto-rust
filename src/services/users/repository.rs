use rust_decimal::Decimal;
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::model::User;
use crate::utils::app_error::AppError;

pub async fn find_user_by_email(
    tx: &mut Transaction<'_, Postgres>,
    email: &str,
) -> Result<Option<User>, AppError> {
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE email = $1 AND deleted_at IS NULL")
        .bind(email)
        .fetch_optional(&mut **tx)
        .await?;
    Ok(user)
}

pub async fn find_user_by_phone(
    tx: &mut Transaction<'_, Postgres>,
    phone: &str,
) -> Result<Option<User>, AppError> {
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE phone = $1 AND deleted_at IS NULL")
        .bind(phone)
        .fetch_optional(&mut **tx)
        .await?;
    Ok(user)
}

pub async fn find_user_by_id(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
) -> Result<Option<User>, AppError> {
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1 AND deleted_at IS NULL")
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?;
    Ok(user)
}

pub async fn create_user(
    tx: &mut Transaction<'_, Postgres>,
    full_name: &str,
    email: &str,
    phone: &str,
    hashed_password: &str,
    role: &str,
) -> Result<User, AppError> {
    let user = sqlx::query_as::<_, User>(
        r#"
        INSERT INTO users (full_name, email, phone, password, role)
        VALUES ($1, $2, $3, $4, $5)
        RETURNING *
        "#,
    )
    .bind(full_name)
    .bind(email)
    .bind(phone)
    .bind(hashed_password)
    .bind(role)
    .fetch_one(&mut **tx)
    .await?;

    Ok(user)
}

#[allow(clippy::too_many_arguments)]
pub async fn update_user(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    full_name: &str,
    email: &str,
    phone: &str,
    platform: Option<&str>,
    api_key: Option<&str>,
    api_secret: Option<&str>,
    photo_id: Option<&str>,
    photo_url: Option<&str>,
    demo_balance: Decimal,
    is_robot_demo_active: bool,
    is_robot_platform_active: bool,
    preferred_currency: &str,
) -> Result<User, AppError> {
    let user = sqlx::query_as::<_, User>(
        r#"
        UPDATE users
        SET full_name = $1, email = $2, phone = $3, platform = $4, api_key = $5,
            api_secret = $6, photo_id = $7, photo_url = $8, demo_balance = $9,
            is_robot_demo_active = $10, is_robot_platform_active = $11, preferred_currency = $12,
            updated_at = now()
        WHERE id = $13 AND deleted_at IS NULL
        RETURNING *
        "#,
    )
    .bind(full_name)
    .bind(email)
    .bind(phone)
    .bind(platform)
    .bind(api_key)
    .bind(api_secret)
    .bind(photo_id)
    .bind(photo_url)
    .bind(demo_balance)
    .bind(is_robot_demo_active)
    .bind(is_robot_platform_active)
    .bind(preferred_currency)
    .bind(user_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| AppError::NotFound("User tidak ditemukan".to_string()))?;

    Ok(user)
}

/// Semua user yang robot demo-nya aktif.
pub async fn find_demo_robot_users(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<Vec<User>, AppError> {
    let users = sqlx::query_as::<_, User>(
        "SELECT * FROM users WHERE is_robot_demo_active = true AND deleted_at IS NULL ORDER BY created_at",
    )
    .fetch_all(&mut **tx)
    .await?;
    Ok(users)
}

/// Simpan hasil 1 putaran robot demo untuk 1 user: saldo demo terbaru, modal awal, tanggal proses,
/// dan status robot (dimatikan kalau saldo tidak cukup / kill switch).
pub async fn update_demo_robot_state(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    demo_balance: Decimal,
    demo_initial_capital: Option<Decimal>,
    last_run_date: chrono::NaiveDate,
    is_robot_demo_active: bool,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        UPDATE users
        SET demo_balance = $1, demo_initial_capital = $2, demo_robot_last_run_date = $3,
            is_robot_demo_active = $4, updated_at = now()
        WHERE id = $5 AND deleted_at IS NULL
        "#,
    )
    .bind(demo_balance)
    .bind(demo_initial_capital)
    .bind(last_run_date)
    .bind(is_robot_demo_active)
    .bind(user_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Semua user yang robot live-nya aktif & punya kredensial exchange lengkap.
pub async fn find_platform_robot_users(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<Vec<User>, AppError> {
    let users = sqlx::query_as::<_, User>(
        r#"
        SELECT * FROM users
        WHERE is_robot_platform_active = true AND deleted_at IS NULL
          AND platform IS NOT NULL AND api_key IS NOT NULL AND api_secret IS NOT NULL
        ORDER BY created_at
        "#,
    )
    .fetch_all(&mut **tx)
    .await?;
    Ok(users)
}

/// Simpan hasil 1 putaran robot live untuk 1 user: modal awal, tanggal proses, dan status robot
/// (dimatikan kalau saldo tidak cukup / kill switch).
pub async fn update_platform_robot_state(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    platform_initial_capital: Option<Decimal>,
    last_run_date: chrono::NaiveDate,
    is_robot_platform_active: bool,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        UPDATE users
        SET platform_initial_capital = $1, platform_robot_last_run_date = $2,
            is_robot_platform_active = $3, updated_at = now()
        WHERE id = $4 AND deleted_at IS NULL
        "#,
    )
    .bind(platform_initial_capital)
    .bind(last_run_date)
    .bind(is_robot_platform_active)
    .bind(user_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Kosongkan modal awal robot live (saat robot diaktifkan ulang) — diisi lagi di putaran berikutnya.
pub async fn reset_platform_initial_capital(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
) -> Result<(), AppError> {
    sqlx::query("UPDATE users SET platform_initial_capital = NULL WHERE id = $1")
        .bind(user_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// Kosongkan modal awal robot demo (saat robot diaktifkan ulang) — diisi lagi di putaran berikutnya.
pub async fn reset_demo_initial_capital(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
) -> Result<(), AppError> {
    sqlx::query("UPDATE users SET demo_initial_capital = NULL WHERE id = $1")
        .bind(user_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}
