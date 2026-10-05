use axum::{
    extract::{Multipart, Path, Query, State},
    response::Response,
    Extension,
};
use sqlx::PgPool;
use uuid::Uuid;

use super::repository::FlexParamScope;
use super::service::{
    create_flex_param_service, delete_flex_param_service, get_flex_param_by_id_service,
    get_flex_params_list_service, update_flex_param_service, PhotoUpload,
};
use super::types::{CreateFlexParamInput, ListQueryOptions, UpdateFlexParamInput};
use crate::utils::api_response::{created, paginated, success, PaginationParams};
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

fn non_empty(value: String) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

/// Terima multipart form-data: field teks sesuai `CreateFlexParamInput`/`UpdateFlexParamInput`,
/// plus field file "photo" opsional.
struct ParsedFlexParamFields {
    type_param: Option<String>,
    value_param: Option<String>,
    description: Option<String>,
    header_id: Option<Uuid>,
    is_active: Option<bool>,
    photo: Option<PhotoUpload>,
}

async fn parse_multipart(mut multipart: Multipart) -> Result<ParsedFlexParamFields, AppError> {
    let mut type_param = None;
    let mut value_param = None;
    let mut description = None;
    let mut header_id = None;
    let mut is_active = None;
    let mut photo: Option<PhotoUpload> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| AppError::BadRequest("Form data tidak valid".to_string()))?
    {
        let name = field.name().unwrap_or("").to_string();

        match name.as_str() {
            "photo" => {
                let original_name = field.file_name().unwrap_or("photo").to_string();
                let mime_type = field
                    .content_type()
                    .unwrap_or("application/octet-stream")
                    .to_string();
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|_| AppError::BadRequest("Gagal membaca foto".to_string()))?;
                photo = Some(PhotoUpload {
                    original_name,
                    mime_type,
                    bytes: bytes.to_vec(),
                });
            }
            "type_param" => type_param = non_empty(field.text().await.unwrap_or_default()),
            "value_param" => value_param = non_empty(field.text().await.unwrap_or_default()),
            "description" => description = non_empty(field.text().await.unwrap_or_default()),
            "header_id" => {
                let value = field.text().await.unwrap_or_default();
                header_id = Uuid::parse_str(&value).ok();
            }
            "is_active" => {
                let value = field.text().await.unwrap_or_default();
                is_active = match value.as_str() {
                    "true" | "1" => Some(true),
                    "false" | "0" => Some(false),
                    _ => None,
                };
            }
            _ => {}
        }
    }

    Ok(ParsedFlexParamFields {
        type_param,
        value_param,
        description,
        header_id,
        is_active,
        photo,
    })
}

pub async fn create_flex_param(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    multipart: Multipart,
) -> Result<Response, AppError> {
    let user_id = parse_user_id(&claims)?;
    let fields = parse_multipart(multipart).await?;

    let is_active = fields
        .is_active
        .ok_or_else(|| AppError::BadRequest("is_active wajib diisi (true/false)".to_string()))?;

    let input = CreateFlexParamInput {
        type_param: fields.type_param.unwrap_or_default(),
        value_param: fields.value_param.unwrap_or_default(),
        description: fields.description,
        header_id: fields.header_id,
        is_active,
    };
    let photo = fields.photo;

    let mut tx = begin_tx(&pool).await?;

    let result = match create_flex_param_service(&mut tx, input, user_id, &claims.email, photo).await
    {
        Ok(result) => result,
        Err(err) => {
            let _ = tx.rollback().await;
            return Err(err);
        }
    };

    commit_tx(tx).await?;
    Ok(created(result, "Flex param berhasil dibuat"))
}

/// Dipakai bersama 3 endpoint list (semua / by type / by header): filter & sort dinamis +
/// pagination, lihat `ListQueryOptions`.
async fn list_flex_params(
    pool: &PgPool,
    scope: FlexParamScope<'_>,
    query: ListQueryOptions,
    message: &str,
) -> Result<Response, AppError> {
    let pagination = PaginationParams::parse(query.page, query.limit);
    let offset = (pagination.page - 1) * pagination.limit;

    let mut tx = begin_tx(pool).await?;

    let (params, total) =
        match get_flex_params_list_service(&mut tx, scope, &query, pagination.limit, offset).await {
            Ok(result) => result,
            Err(err) => {
                let _ = tx.rollback().await;
                return Err(err);
            }
        };

    commit_tx(tx).await?;
    Ok(paginated(params, total, &pagination, message))
}

pub async fn get_all_flex_params(
    State(pool): State<PgPool>,
    Query(query): Query<ListQueryOptions>,
) -> Result<Response, AppError> {
    list_flex_params(&pool, FlexParamScope::All, query, "Berhasil mengambil data flex params").await
}

pub async fn get_flex_param_by_id(
    State(pool): State<PgPool>,
    Path(id): Path<Uuid>,
) -> Result<Response, AppError> {
    let mut tx = begin_tx(&pool).await?;

    let result = match get_flex_param_by_id_service(&mut tx, id).await {
        Ok(result) => result,
        Err(err) => {
            let _ = tx.rollback().await;
            return Err(err);
        }
    };

    commit_tx(tx).await?;
    Ok(success(result, "Berhasil mengambil flex param"))
}

pub async fn get_flex_params_by_type(
    State(pool): State<PgPool>,
    Path(type_param): Path<String>,
    Query(query): Query<ListQueryOptions>,
) -> Result<Response, AppError> {
    list_flex_params(
        &pool,
        FlexParamScope::Type(&type_param),
        query,
        "Berhasil mengambil flex params by type",
    )
    .await
}

pub async fn get_flex_params_by_header_id(
    State(pool): State<PgPool>,
    Path(header_id): Path<Uuid>,
    Query(query): Query<ListQueryOptions>,
) -> Result<Response, AppError> {
    list_flex_params(
        &pool,
        FlexParamScope::Header(header_id),
        query,
        "Berhasil mengambil flex params by header id",
    )
    .await
}

pub async fn update_flex_param(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Path(id): Path<Uuid>,
    multipart: Multipart,
) -> Result<Response, AppError> {
    let fields = parse_multipart(multipart).await?;

    let input = UpdateFlexParamInput {
        type_param: fields.type_param,
        value_param: fields.value_param,
        description: fields.description,
        header_id: fields.header_id,
        is_active: fields.is_active,
    };

    let mut tx = begin_tx(&pool).await?;

    let result = match update_flex_param_service(&mut tx, id, input, &claims.email, fields.photo).await {
        Ok(result) => result,
        Err(err) => {
            let _ = tx.rollback().await;
            return Err(err);
        }
    };

    commit_tx(tx).await?;
    Ok(success(result, "Flex param berhasil diupdate"))
}

pub async fn delete_flex_param(
    State(pool): State<PgPool>,
    Extension(claims): Extension<JwtClaims>,
    Path(id): Path<Uuid>,
) -> Result<Response, AppError> {
    let mut tx = begin_tx(&pool).await?;

    if let Err(err) = delete_flex_param_service(&mut tx, id, &claims.email).await {
        let _ = tx.rollback().await;
        return Err(err);
    }

    commit_tx(tx).await?;
    Ok(success(serde_json::Value::Null, "Flex param berhasil dihapus"))
}
