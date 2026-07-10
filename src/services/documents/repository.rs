use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::model::FileRecord;
use super::types::FileMetaInput;
use crate::utils::app_error::AppError;

pub async fn find_file_by_id(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
) -> Result<Option<FileRecord>, AppError> {
    let file = sqlx::query_as::<_, FileRecord>(
        "SELECT * FROM files WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(file)
}

#[allow(clippy::too_many_arguments)]
pub async fn create_file(
    tx: &mut Transaction<'_, Postgres>,
    file_name: &str,
    file_url: &str,
    file_type: &str,
    user_id: Uuid,
    meta: &FileMetaInput,
    created_by: &str,
) -> Result<FileRecord, AppError> {
    let file = sqlx::query_as::<_, FileRecord>(
        r#"
        INSERT INTO files (file_name, file_url, file_type, user_id, ref_id, ref_type, created_by, updated_by)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $7)
        RETURNING *
        "#,
    )
    .bind(file_name)
    .bind(file_url)
    .bind(file_type)
    .bind(user_id)
    .bind(meta.ref_id)
    .bind(&meta.ref_type)
    .bind(created_by)
    .fetch_one(&mut **tx)
    .await?;

    Ok(file)
}

#[allow(clippy::too_many_arguments)]
pub async fn update_file(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    file_name: &str,
    file_url: &str,
    file_type: &str,
    meta: &FileMetaInput,
    updated_by: &str,
) -> Result<FileRecord, AppError> {
    let file = sqlx::query_as::<_, FileRecord>(
        r#"
        UPDATE files
        SET file_name = $1, file_url = $2, file_type = $3, ref_id = $4, ref_type = $5,
            updated_by = $6, updated_at = now()
        WHERE id = $7 AND deleted_at IS NULL
        RETURNING *
        "#,
    )
    .bind(file_name)
    .bind(file_url)
    .bind(file_type)
    .bind(meta.ref_id)
    .bind(&meta.ref_type)
    .bind(updated_by)
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| AppError::NotFound("Dokumen tidak ditemukan".to_string()))?;

    Ok(file)
}

pub async fn soft_delete_file(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    deleted_by: &str,
) -> Result<(), AppError> {
    let result = sqlx::query(
        r#"
        UPDATE files
        SET deleted_by = $1, deleted_at = now()
        WHERE id = $2 AND deleted_at IS NULL
        "#,
    )
    .bind(deleted_by)
    .bind(id)
    .execute(&mut **tx)
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Dokumen tidak ditemukan".to_string()));
    }

    Ok(())
}
