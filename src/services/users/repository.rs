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
) -> Result<User, AppError> {
    let user = sqlx::query_as::<_, User>(
        r#"
        UPDATE users
        SET full_name = $1, email = $2, phone = $3, platform = $4, api_key = $5,
            api_secret = $6, photo_id = $7, photo_url = $8, demo_balance = $9,
            is_robot_demo_active = $10, is_robot_platform_active = $11, updated_at = now()
        WHERE id = $12 AND deleted_at IS NULL
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
    .bind(user_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| AppError::NotFound("User tidak ditemukan".to_string()))?;

    Ok(user)
}
