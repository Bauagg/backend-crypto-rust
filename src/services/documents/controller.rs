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
    let user_id = claims.user_uuid()?;
    let (upload, meta) = parse_multipart(multipart).await?;

    let upload = upload.ok_or_else(|| AppError::BadRequest("File wajib diupload".to_string()))?;

    let result = upload_file_service(&pool, upload, user_id, meta, &claims.email).await?;
    Ok(created(result, "Dokumen berhasil diupload"))
}

pub async fn get_file(
    State(pool): State<PgPool>,
    Path(id): Path<Uuid>,
) -> Result<Response, AppError> {
    let result = get_file_service(&pool, id).await?;
    Ok(success(result, "Berhasil mengambil dokumen"))
}

pub async fn update_file(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Path(id): Path<Uuid>,
    multipart: Multipart,
) -> Result<Response, AppError> {
    let (upload, meta) = parse_multipart(multipart).await?;

    let result = update_file_service(&pool, id, upload, meta, &claims.email).await?;
    Ok(success(result, "Dokumen berhasil diupdate"))
}

pub async fn delete_file(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Path(id): Path<Uuid>,
) -> Result<Response, AppError> {
    delete_file_service(&pool, id, &claims.email).await?;
    Ok(success(serde_json::Value::Null, "Dokumen berhasil dihapus"))
}
