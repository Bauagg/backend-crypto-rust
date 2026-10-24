use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::model::UserScreener;
use crate::utils::app_error::AppError;

/// Strategi milik user (1 user hanya punya 1 strategi aktif).
pub async fn find_screener_of_user(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
) -> Result<Option<UserScreener>, AppError> {
    let screener = sqlx::query_as::<_, UserScreener>(
        "SELECT * FROM user_screeners WHERE user_id = $1 AND deleted_at IS NULL",
    )
    .bind(user_id)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(screener)
}

pub struct ScreenerFields<'a> {
    pub name: &'a str,
    pub description: Option<&'a str>,
    pub max_results: i16,
    pub min_score: i16,
    pub is_active: bool,
}

/// Buat strategi user, atau ganti isinya kalau sudah ada — 1 query atomik (`ON CONFLICT` ke index
/// unik `user_id`), jadi 2 request bersamaan tetap tidak bisa membuat 2 strategi.
pub async fn upsert_screener(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    fields: &ScreenerFields<'_>,
    actor: &str,
) -> Result<UserScreener, AppError> {
    let screener = sqlx::query_as::<_, UserScreener>(
        r#"
        INSERT INTO user_screeners
            (user_id, name, description, max_results, min_score, is_active, created_by, updated_by)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $7)
        ON CONFLICT (user_id) WHERE deleted_at IS NULL DO UPDATE
        SET name = EXCLUDED.name, description = EXCLUDED.description,
            max_results = EXCLUDED.max_results, min_score = EXCLUDED.min_score,
            is_active = EXCLUDED.is_active, updated_by = EXCLUDED.updated_by, updated_at = now()
        RETURNING *
        "#,
    )
    .bind(user_id)
    .bind(fields.name)
    .bind(fields.description)
    .bind(fields.max_results)
    .bind(fields.min_score)
    .bind(fields.is_active)
    .bind(actor)
    .fetch_one(&mut **tx)
    .await?;
    Ok(screener)
}

pub async fn soft_delete_screener(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    actor: &str,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        UPDATE user_screeners SET deleted_at = now(), deleted_by = $2, updated_at = now(), updated_by = $2
        WHERE id = $1 AND deleted_at IS NULL
        "#,
    )
    .bind(id)
    .bind(actor)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
