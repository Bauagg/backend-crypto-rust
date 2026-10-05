use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use super::model::FlexParam;
use super::repository::{
    create_flex_param, find_flex_param_by_id, find_flex_param_by_type_and_value,
    find_flex_params_paginated, soft_delete_flex_param, update_flex_param, FlexParamScope,
};
use super::types::{CreateFlexParamInput, ListQueryOptions, UpdateFlexParamInput};
use crate::services::documents::service::{delete_file_service, update_file_service, upload_file_service, UploadInput};
use crate::services::documents::types::FileMetaInput;
use crate::utils::app_error::AppError;
use crate::utils::query_filter::{parse_filter_query, parse_sort_query, ColumnKind, FilterColumn};
use validator::Validate;

/// Bytes foto opsional yang menyertai create/update flex param, diteruskan ke `files::service`.
pub struct PhotoUpload {
    pub original_name: String,
    pub mime_type: String,
    pub bytes: Vec<u8>,
}

pub async fn create_flex_param_service(
    tx: &mut Transaction<'_, Postgres>,
    input: CreateFlexParamInput,
    user_id: Uuid,
    created_by: &str,
    photo: Option<PhotoUpload>,
) -> Result<FlexParam, AppError> {
    input.validate()?;

    let duplicate =
        find_flex_param_by_type_and_value(tx, &input.type_param, &input.value_param, None).await?;
    if duplicate.is_some() {
        return Err(AppError::Conflict(format!(
            "{} sudah ada di tipe {}",
            input.value_param, input.type_param
        )));
    }

    let (photo_id, photo_url) = match photo {
        Some(photo) => {
            let upload = UploadInput {
                original_name: photo.original_name,
                mime_type: photo.mime_type,
                bytes: photo.bytes,
            };
            let meta = FileMetaInput::default();
            let file = upload_file_service(tx, upload, user_id, meta, created_by).await?;
            (Some(file.id), Some(file.file_url))
        }
        None => (None, None),
    };

    create_flex_param(
        tx,
        &input.type_param,
        &input.value_param,
        input.description.as_deref(),
        user_id,
        input.header_id,
        photo_id,
        photo_url.as_deref(),
        input.is_active,
        created_by,
    )
    .await
}

/// Kolom yang boleh dipakai di `?filter=` & `?sort=` endpoint list flex params.
const FLEX_PARAM_COLUMNS: [FilterColumn; 13] = [
    FilterColumn::new("id", ColumnKind::Uuid),
    FilterColumn::new("type_param", ColumnKind::Text),
    FilterColumn::new("value_param", ColumnKind::Text),
    FilterColumn::new("description", ColumnKind::Text),
    FilterColumn::new("user_id", ColumnKind::Uuid),
    FilterColumn::new("photo_id", ColumnKind::Uuid),
    FilterColumn::new("photo_url", ColumnKind::Text),
    FilterColumn::new("header_id", ColumnKind::Uuid),
    FilterColumn::new("is_active", ColumnKind::Bool),
    FilterColumn::new("created_at", ColumnKind::Timestamp),
    FilterColumn::new("created_by", ColumnKind::Text),
    FilterColumn::new("updated_at", ColumnKind::Timestamp),
    FilterColumn::new("updated_by", ColumnKind::Text),
];

/// List flex params dengan filter, sort & pagination. Kolom yang sudah dikunci lewat path URL
/// (`type_param` di `/type/:type_param`, `header_id` di `/header/:header_id`) tidak boleh
/// difilter/diurutkan lagi lewat query.
pub async fn get_flex_params_list_service(
    tx: &mut Transaction<'_, Postgres>,
    scope: FlexParamScope<'_>,
    query: &ListQueryOptions,
    limit: i64,
    offset: i64,
) -> Result<(Vec<FlexParam>, i64), AppError> {
    let (locked_column, default_order) = match scope {
        FlexParamScope::All => (None, "created_at DESC"),
        FlexParamScope::Type(_) => (Some("type_param"), "value_param ASC"),
        FlexParamScope::Header(_) => (Some("header_id"), "created_at DESC"),
    };
    let columns: Vec<FilterColumn> = FLEX_PARAM_COLUMNS
        .into_iter()
        .filter(|c| Some(c.name) != locked_column)
        .collect();

    let filters = parse_filter_query(query.filter.as_deref())?;
    let sort = parse_sort_query(query.sort.as_deref(), query.order.as_deref())?;

    find_flex_params_paginated(
        tx,
        scope,
        &filters,
        sort.as_ref(),
        &columns,
        default_order,
        limit,
        offset,
    )
    .await
}

pub async fn get_flex_param_by_id_service(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
) -> Result<FlexParam, AppError> {
    find_flex_param_by_id(tx, id)
        .await?
        .ok_or_else(|| AppError::NotFound("Flex param tidak ditemukan".to_string()))
}

pub async fn update_flex_param_service(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    input: UpdateFlexParamInput,
    updated_by: &str,
    photo: Option<PhotoUpload>,
) -> Result<FlexParam, AppError> {
    input.validate()?;

    let existing = find_flex_param_by_id(tx, id)
        .await?
        .ok_or_else(|| AppError::NotFound("Flex param tidak ditemukan".to_string()))?;

    let type_param = input.type_param.unwrap_or(existing.type_param.clone());
    let value_param = input.value_param.clone();

    if let Some(new_value) = &value_param {
        let duplicate =
            find_flex_param_by_type_and_value(tx, &type_param, new_value, Some(id)).await?;
        if duplicate.is_some() {
            return Err(AppError::Conflict(format!(
                "{new_value} sudah ada di tipe {type_param}"
            )));
        }
    }

    let value_param = value_param.unwrap_or(existing.value_param);
    let description = input.description.or(existing.description);
    let header_id = input.header_id.or(existing.header_id);

    let (photo_id, photo_url) = match photo {
        Some(photo) => {
            let upload = UploadInput {
                original_name: photo.original_name,
                mime_type: photo.mime_type,
                bytes: photo.bytes,
            };

            let file = match existing.photo_id {
                Some(file_id) => {
                    update_file_service(tx, file_id, Some(upload), FileMetaInput::default(), updated_by)
                        .await?
                }
                None => {
                    upload_file_service(tx, upload, existing.user_id, FileMetaInput::default(), updated_by)
                        .await?
                }
            };

            (Some(file.id), Some(file.file_url))
        }
        None => (existing.photo_id, existing.photo_url),
    };

    let is_active = input.is_active.unwrap_or(existing.is_active);

    update_flex_param(
        tx,
        id,
        &type_param,
        &value_param,
        description.as_deref(),
        header_id,
        photo_id,
        photo_url.as_deref(),
        is_active,
        updated_by,
    )
    .await
}

pub async fn delete_flex_param_service(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    deleted_by: &str,
) -> Result<(), AppError> {
    let existing = find_flex_param_by_id(tx, id)
        .await?
        .ok_or_else(|| AppError::NotFound("Flex param tidak ditemukan".to_string()))?;

    if let Some(photo_id) = existing.photo_id {
        delete_file_service(tx, photo_id, deleted_by).await?;
    }

    soft_delete_flex_param(tx, id, deleted_by).await
}
