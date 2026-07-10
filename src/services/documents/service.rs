use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::model::FileRecord;
use super::repository::{create_file, find_file_by_id, soft_delete_file, update_file};
use super::types::FileMetaInput;
use crate::utils::app_error::AppError;

fn upload_dir() -> std::path::PathBuf {
    let folder = std::env::var("PATH_FILE_UPLOAD").unwrap_or_else(|_| "files".to_string());
    std::path::PathBuf::from(folder.trim())
}

fn base_url() -> String {
    std::env::var("BASE_URL").unwrap_or_else(|_| "http://localhost:3000".to_string())
}

/// Simpan bytes file ke disk dengan nama unik (UUID + extension asli), lalu kembalikan URL publiknya.
fn save_file_to_disk(original_name: &str, bytes: &[u8]) -> Result<String, AppError> {
    let dir = upload_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|_| AppError::Internal("Gagal membuat folder upload".to_string()))?;

    let ext = std::path::Path::new(original_name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    let unique_name = if ext.is_empty() {
        Uuid::new_v4().to_string()
    } else {
        format!("{}.{}", Uuid::new_v4(), ext)
    };

    let file_path = dir.join(&unique_name);
    std::fs::write(&file_path, bytes)
        .map_err(|_| AppError::Internal("Gagal menyimpan file ke disk".to_string()))?;

    let upload_folder = std::env::var("PATH_FILE_UPLOAD").unwrap_or_else(|_| "files".to_string());
    let upload_folder = upload_folder.trim();
    Ok(format!("{}/{}/{}", base_url(), upload_folder, unique_name))
}

/// Hapus file fisik dari disk berdasarkan URL publiknya. Tidak error kalau file sudah tidak ada.
fn delete_file_from_disk(file_url: &str) {
    let Some(unique_name) = file_url.rsplit('/').next() else {
        return;
    };
    let path = upload_dir().join(unique_name);
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
}

pub struct UploadInput {
    pub original_name: String,
    pub mime_type: String,
    pub bytes: Vec<u8>,
}

pub async fn upload_file_service(
    tx: &mut Transaction<'_, Postgres>,
    upload: UploadInput,
    user_id: Uuid,
    meta: FileMetaInput,
    created_by: &str,
) -> Result<FileRecord, AppError> {
    let file_url = save_file_to_disk(&upload.original_name, &upload.bytes)?;

    create_file(
        tx,
        &upload.original_name,
        &file_url,
        &upload.mime_type,
        user_id,
        &meta,
        created_by,
    )
    .await
}

pub async fn get_file_service(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
) -> Result<FileRecord, AppError> {
    find_file_by_id(tx, id)
        .await?
        .ok_or_else(|| AppError::NotFound("Dokumen tidak ditemukan".to_string()))
}

pub async fn update_file_service(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    new_upload: Option<UploadInput>,
    meta: FileMetaInput,
    updated_by: &str,
) -> Result<FileRecord, AppError> {
    let existing = find_file_by_id(tx, id)
        .await?
        .ok_or_else(|| AppError::NotFound("Dokumen tidak ditemukan".to_string()))?;

    let meta = FileMetaInput {
        ref_id: meta.ref_id.or(existing.ref_id),
        ref_type: meta.ref_type.or(existing.ref_type.clone()),
    };

    let (file_name, file_url, file_type) = match new_upload {
        Some(upload) => {
            delete_file_from_disk(&existing.file_url);
            let new_url = save_file_to_disk(&upload.original_name, &upload.bytes)?;
            (upload.original_name, new_url, upload.mime_type)
        }
        None => (existing.file_name, existing.file_url, existing.file_type),
    };

    update_file(tx, id, &file_name, &file_url, &file_type, &meta, updated_by).await
}

pub async fn delete_file_service(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    deleted_by: &str,
) -> Result<(), AppError> {
    let existing = find_file_by_id(tx, id)
        .await?
        .ok_or_else(|| AppError::NotFound("Dokumen tidak ditemukan".to_string()))?;

    soft_delete_file(tx, id, deleted_by).await?;
    delete_file_from_disk(&existing.file_url);

    Ok(())
}
