use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::model::Strategy;
use crate::utils::app_error::AppError;

pub async fn find_strategy_by_id(tx: &mut Transaction<'_, Postgres>, id: Uuid) -> Result<Option<Strategy>, AppError> {
    let strategy = sqlx::query_as::<_, Strategy>("SELECT * FROM strategies WHERE id = $1 AND deleted_at IS NULL")
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?;
    Ok(strategy)
}

/// Beberapa strategi sekaligus (termasuk yang sudah dihapus — untuk menampilkan nama di riwayat).
pub async fn find_strategies_by_ids(tx: &mut Transaction<'_, Postgres>, ids: &[Uuid]) -> Result<Vec<Strategy>, AppError> {
    let strategies = sqlx::query_as::<_, Strategy>("SELECT * FROM strategies WHERE id = ANY($1)")
        .bind(ids)
        .fetch_all(&mut **tx)
        .await?;
    Ok(strategies)
}

pub async fn find_strategies_of_user(tx: &mut Transaction<'_, Postgres>, user_id: Uuid) -> Result<Vec<Strategy>, AppError> {
    let strategies = sqlx::query_as::<_, Strategy>(
        "SELECT * FROM strategies WHERE user_id = $1 AND deleted_at IS NULL ORDER BY created_at DESC",
    )
    .bind(user_id)
    .fetch_all(&mut **tx)
    .await?;
    Ok(strategies)
}

pub async fn count_strategies_of_user(tx: &mut Transaction<'_, Postgres>, user_id: Uuid) -> Result<i64, AppError> {
    let count = sqlx::query_scalar("SELECT COUNT(*) FROM strategies WHERE user_id = $1 AND deleted_at IS NULL")
        .bind(user_id)
        .fetch_one(&mut **tx)
        .await?;
    Ok(count)
}

/// Ada strategi lain milik user dengan nama ini (tidak peduli huruf besar/kecil)?
pub async fn exists_strategy_name(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    name: &str,
    exclude_id: Option<Uuid>,
) -> Result<bool, AppError> {
    let exists = sqlx::query_scalar(
        r#"
        SELECT EXISTS (
            SELECT 1 FROM strategies
            WHERE user_id = $1 AND lower(name) = lower($2) AND deleted_at IS NULL
              AND ($3::uuid IS NULL OR id <> $3)
        )
        "#,
    )
    .bind(user_id)
    .bind(name)
    .bind(exclude_id)
    .fetch_one(&mut **tx)
    .await?;
    Ok(exists)
}

pub struct StrategyFields<'a> {
    pub name: &'a str,
    pub description: Option<&'a str>,
    pub kind: &'a str,
    pub visibility: &'a str,
    pub status: &'a str,
}

pub async fn create_strategy(
    tx: &mut Transaction<'_, Postgres>,
    user_id: Uuid,
    fields: &StrategyFields<'_>,
    actor: &str,
) -> Result<Strategy, AppError> {
    let strategy = sqlx::query_as::<_, Strategy>(
        r#"
        INSERT INTO strategies (user_id, name, description, kind, visibility, status, created_by, updated_by)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $7)
        RETURNING *
        "#,
    )
    .bind(user_id)
    .bind(fields.name)
    .bind(fields.description)
    .bind(fields.kind)
    .bind(fields.visibility)
    .bind(fields.status)
    .bind(actor)
    .fetch_one(&mut **tx)
    .await?;
    Ok(strategy)
}

pub async fn update_strategy(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    fields: &StrategyFields<'_>,
    actor: &str,
) -> Result<Strategy, AppError> {
    let strategy = sqlx::query_as::<_, Strategy>(
        r#"
        UPDATE strategies
        SET name = $2, description = $3, kind = $4, visibility = $5, status = $6,
            updated_by = $7, updated_at = now()
        WHERE id = $1 AND deleted_at IS NULL
        RETURNING *
        "#,
    )
    .bind(id)
    .bind(fields.name)
    .bind(fields.description)
    .bind(fields.kind)
    .bind(fields.visibility)
    .bind(fields.status)
    .bind(actor)
    .fetch_one(&mut **tx)
    .await?;
    Ok(strategy)
}

pub async fn set_current_version(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    version_id: Uuid,
    actor: &str,
) -> Result<Strategy, AppError> {
    let strategy = sqlx::query_as::<_, Strategy>(
        r#"
        UPDATE strategies SET current_version_id = $2, updated_by = $3, updated_at = now()
        WHERE id = $1 AND deleted_at IS NULL
        RETURNING *
        "#,
    )
    .bind(id)
    .bind(version_id)
    .bind(actor)
    .fetch_one(&mut **tx)
    .await?;
    Ok(strategy)
}

pub async fn soft_delete_strategy(tx: &mut Transaction<'_, Postgres>, id: Uuid, actor: &str) -> Result<(), AppError> {
    sqlx::query(
        r#"
        UPDATE strategies SET deleted_at = now(), deleted_by = $2, updated_at = now(), updated_by = $2
        WHERE id = $1 AND deleted_at IS NULL
        "#,
    )
    .bind(id)
    .bind(actor)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Atur `visibility` (PUBLIC saat dipasang di Store, PRIVATE saat dicabut).
pub async fn set_visibility(tx: &mut Transaction<'_, Postgres>, id: Uuid, visibility: &str, actor: &str) -> Result<(), AppError> {
    sqlx::query("UPDATE strategies SET visibility = $2, updated_by = $3, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(visibility)
        .bind(actor)
        .execute(&mut **tx)
        .await?;
    Ok(())
}
