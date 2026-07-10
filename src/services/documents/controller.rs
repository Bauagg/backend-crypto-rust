use axum::{
    extract::{Multipart, Path, State},
    response::Response,
    Extension,
};
use sqlx::PgPool;
use uuid::Uuid;

use super::service::{
    delete_file_service, get_file_service, update_file_service, upload_file_service, UploadInput,
};
use super::types::FileMetaInput;
use crate::utils::api_response::{created, success};
use crate::utils::app_error::AppError;
use crate::utils::jwt::JwtClaims;

fn parse_user_id(claims: &JwtClaims) -> Result<Uuid, AppError> {
    Uuid::parse_str(&claims.user_id).map_err(|_| AppError::Unauthorized("Token tidak valid".to_string()))
}

async fn begin_tx(pool: &PgPool) -> Result<sqlx::Transaction<'_, sqlx::Postgres>, AppError> {
    pool.begin()
        .await
        .map_err(|_| AppError::Internal("Gagal memulai transaksi".to_string()))
}

async fn commit_tx(tx: sqlx::Transaction<'_, sqlx::Postgres>) -> Result<(), AppError> {
    tx.commit()
        .await
        .map_err(|_| AppError::Internal("Gagal menyimpan perubahan".to_string()))
}

/// Ekstrak field "file" (bytes) + field teks "ref_id"/"ref_type" dari multipart form-data.
async fn parse_multipart(
    mut multipart: Multipart,
) -> Result<(Option<UploadInput>, FileMetaInput), AppError> {
    let mut upload: Option<UploadInput> = None;
    let mut meta = FileMetaInput::default();

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| AppError::BadRequest("Form data tidak valid".to_string()))?
    {
        let name = field.name().unwrap_or("").to_string();

        match name.as_str() {
            "file" => {
                let original_name = field.file_name().unwrap_or("file").to_string();
                let mime_type = field
                    .content_type()
                    .unwrap_or("application/octet-stream")
                    .to_string();
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|_| AppError::BadRequest("Gagal membaca file".to_string()))?;

                upload = Some(UploadInput {
                    original_name,
                    mime_type,
                    bytes: bytes.to_vec(),
                });
            }
            "ref_id" => {
                let value = field.text().await.unwrap_or_default();
                if !value.is_empty() {
                    meta.ref_id = Uuid::parse_str(&value).ok();
                }
            }
            "ref_type" => {
                let value = field.text().await.unwrap_or_default();
                if !value.is_empty() {
                    meta.ref_type = Some(value);
                }
            }
            _ => {}
        }
    }

    Ok((upload, meta))
}

pub async fn upload_file(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    multipart: Multipart,
) -> Result<Response, AppError> {
    let user_id = parse_user_id(&claims)?;
    let (upload, meta) = parse_multipart(multipart).await?;

    let upload = upload.ok_or_else(|| AppError::BadRequest("File wajib diupload".to_string()))?;

    let mut tx = begin_tx(&pool).await?;

    let result = match upload_file_service(&mut tx, upload, user_id, meta, &claims.email).await {
        Ok(result) => result,
        Err(err) => {
            let _ = tx.rollback().await;
            return Err(err);
        }
    };

    commit_tx(tx).await?;
    Ok(created(result, "Dokumen berhasil diupload"))
}

pub async fn get_file(
    State(pool): State<PgPool>,
    Path(id): Path<Uuid>,
) -> Result<Response, AppError> {
    let mut tx = begin_tx(&pool).await?;

    let result = match get_file_service(&mut tx, id).await {
        Ok(result) => result,
        Err(err) => {
            let _ = tx.rollback().await;
            return Err(err);
        }
    };

    commit_tx(tx).await?;
    Ok(success(result, "Berhasil mengambil dokumen"))
}

pub async fn update_file(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Path(id): Path<Uuid>,
    multipart: Multipart,
) -> Result<Response, AppError> {
    let (upload, meta) = parse_multipart(multipart).await?;

    let mut tx = begin_tx(&pool).await?;

    let result = match update_file_service(&mut tx, id, upload, meta, &claims.email).await {
        Ok(result) => result,
        Err(err) => {
            let _ = tx.rollback().await;
            return Err(err);
        }
    };

    commit_tx(tx).await?;
    Ok(success(result, "Dokumen berhasil diupdate"))
}

pub async fn delete_file(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Path(id): Path<Uuid>,
) -> Result<Response, AppError> {
    let mut tx = begin_tx(&pool).await?;

    if let Err(err) = delete_file_service(&mut tx, id, &claims.email).await {
        let _ = tx.rollback().await;
        return Err(err);
    }

    commit_tx(tx).await?;
    Ok(success(serde_json::Value::Null, "Dokumen berhasil dihapus"))
}
