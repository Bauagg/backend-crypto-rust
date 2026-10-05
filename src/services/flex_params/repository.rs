use sqlx::{Postgres, QueryBuilder, Transaction};
use uuid::Uuid;

use super::model::FlexParam;
use crate::utils::app_error::AppError;
use crate::utils::query_filter::{
    push_filters, push_order_by, FilterColumn, FilterCondition, SortCondition,
};

pub async fn find_flex_param_by_type_and_value(
    tx: &mut Transaction<'_, Postgres>,
    type_param: &str,
    value_param: &str,
    exclude_id: Option<Uuid>,
) -> Result<Option<FlexParam>, AppError> {
    let param = sqlx::query_as::<_, FlexParam>(
        r#"
        SELECT * FROM flex_params
        WHERE type_param ILIKE $1 AND value_param ILIKE $2 AND deleted_at IS NULL
          AND ($3::uuid IS NULL OR id != $3)
        "#,
    )
    .bind(type_param)
    .bind(value_param)
    .bind(exclude_id)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(param)
}

#[allow(clippy::too_many_arguments)]
pub async fn find_all_flex_params(
    tx: &mut Transaction<'_, Postgres>,
    type_param: Option<&str>,
    search: Option<&str>,
    only_active: bool,
    limit: i64,
    offset: i64,
) -> Result<Vec<FlexParam>, AppError> {
    let search_pattern = search.map(|s| format!("%{s}%"));

    let params = sqlx::query_as::<_, FlexParam>(
        r#"
        SELECT * FROM flex_params
        WHERE deleted_at IS NULL
          AND ($1::text IS NULL OR type_param = $1)
          AND ($2::text IS NULL OR value_param ILIKE $2)
          AND ($3 = false OR is_active = true)
        ORDER BY created_at DESC
        LIMIT $4 OFFSET $5
        "#,
    )
    .bind(type_param)
    .bind(&search_pattern)
    .bind(only_active)
    .bind(limit)
    .bind(offset)
    .fetch_all(&mut **tx)
    .await?;
    Ok(params)
}

pub async fn count_flex_params(
    tx: &mut Transaction<'_, Postgres>,
    type_param: Option<&str>,
    search: Option<&str>,
    only_active: bool,
) -> Result<i64, AppError> {
    let search_pattern = search.map(|s| format!("%{s}%"));

    let total: (i64,) = sqlx::query_as(
        r#"
        SELECT COUNT(*) FROM flex_params
        WHERE deleted_at IS NULL
          AND ($1::text IS NULL OR type_param = $1)
          AND ($2::text IS NULL OR value_param ILIKE $2)
          AND ($3 = false OR is_active = true)
        "#,
    )
    .bind(type_param)
    .bind(&search_pattern)
    .bind(only_active)
    .fetch_one(&mut **tx)
    .await?;

    Ok(total.0)
}

pub async fn find_flex_param_by_id(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
) -> Result<Option<FlexParam>, AppError> {
    let param = sqlx::query_as::<_, FlexParam>(
        "SELECT * FROM flex_params WHERE id = $1 AND deleted_at IS NULL",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(param)
}

/// `only_active = true` -> hanya baris yang is_active. `only_active = false` -> semua baris
/// (aktif maupun tidak aktif), tidak difilter berdasarkan status.
pub async fn find_flex_params_by_type(
    tx: &mut Transaction<'_, Postgres>,
    type_param: &str,
    only_active: bool,
) -> Result<Vec<FlexParam>, AppError> {
    let params = sqlx::query_as::<_, FlexParam>(
        r#"
        SELECT * FROM flex_params
        WHERE type_param = $1 AND deleted_at IS NULL
          AND ($2 = false OR is_active = true)
        ORDER BY value_param ASC
        "#,
    )
    .bind(type_param)
    .bind(only_active)
    .fetch_all(&mut **tx)
    .await?;
    Ok(params)
}

/// Baris yang sudah di-soft-delete untuk 1 tipe, sebagai `(id, value_param, deleted_by, punya_foto)`
/// — terbaru dulu per `value_param`. Dipakai sync otomatis (mis. `coin_symbols`) untuk membedakan
/// baris yang dihapus sistem (boleh dipulihkan) dari yang dihapus user (tidak boleh dibuat ulang).
pub async fn find_deleted_flex_params_by_type(
    tx: &mut Transaction<'_, Postgres>,
    type_param: &str,
) -> Result<Vec<(Uuid, String, Option<String>, bool)>, AppError> {
    let rows = sqlx::query_as(
        r#"
        SELECT id, value_param, deleted_by, photo_url IS NOT NULL FROM flex_params
        WHERE type_param = $1 AND deleted_at IS NOT NULL
        ORDER BY value_param ASC, deleted_at DESC
        "#,
    )
    .bind(type_param)
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows)
}

/// `value_param` 1 tipe yang masih hidup **plus** yang di-soft-delete dalam `deleted_within_days`
/// hari terakhir, sebagai `(value_param, sudah_dihapus)`. Dipakai worker candle supaya data coin
/// yang baru keluar dari daftar tetap terkumpul sebentar (mis. untuk evaluasi rekomendasi lama).
pub async fn find_flex_param_values_with_recently_deleted(
    tx: &mut Transaction<'_, Postgres>,
    type_param: &str,
    deleted_within_days: i32,
) -> Result<Vec<(String, bool)>, AppError> {
    let rows = sqlx::query_as(
        r#"
        SELECT value_param, bool_and(deleted_at IS NOT NULL) AS is_deleted
        FROM flex_params
        WHERE type_param = $1
          AND (deleted_at IS NULL OR deleted_at > now() - make_interval(days => $2))
        GROUP BY value_param
        ORDER BY value_param ASC
        "#,
    )
    .bind(type_param)
    .bind(deleted_within_days)
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows)
}

/// Batalkan soft delete (`deleted_at`/`deleted_by` dikosongkan) — foto & data lain tetap utuh.
pub async fn restore_flex_param(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    is_active: bool,
    updated_by: &str,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        UPDATE flex_params
        SET deleted_at = NULL, deleted_by = NULL, is_active = $1, updated_by = $2, updated_at = now()
        WHERE id = $3 AND deleted_at IS NOT NULL
        "#,
    )
    .bind(is_active)
    .bind(updated_by)
    .bind(id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Cakupan data list: semua, atau dikunci ke 1 `type_param` / 1 `header_id` dari path URL.
pub enum FlexParamScope<'a> {
    All,
    Type(&'a str),
    Header(Uuid),
}

/// `WHERE` dasar (belum dihapus + cakupan) lalu filter dinamis dari user.
fn push_flex_param_conditions(
    builder: &mut QueryBuilder<'_, Postgres>,
    scope: &FlexParamScope<'_>,
    filters: &[FilterCondition],
    columns: &[FilterColumn],
) -> Result<(), AppError> {
    builder.push(" WHERE deleted_at IS NULL");
    match scope {
        FlexParamScope::All => {}
        FlexParamScope::Type(type_param) => {
            builder.push(" AND type_param = ").push_bind(type_param.to_string());
        }
        FlexParamScope::Header(header_id) => {
            builder.push(" AND header_id = ").push_bind(*header_id);
        }
    }
    push_filters(builder, filters, columns)
}

/// List flex params dengan filter & sort dinamis + pagination. Balik `(data halaman ini, total)`.
#[allow(clippy::too_many_arguments)]
pub async fn find_flex_params_paginated(
    tx: &mut Transaction<'_, Postgres>,
    scope: FlexParamScope<'_>,
    filters: &[FilterCondition],
    sort: Option<&SortCondition>,
    columns: &[FilterColumn],
    default_order: &str,
    limit: i64,
    offset: i64,
) -> Result<(Vec<FlexParam>, i64), AppError> {
    let mut query = QueryBuilder::<Postgres>::new("SELECT * FROM flex_params");
    push_flex_param_conditions(&mut query, &scope, filters, columns)?;
    push_order_by(&mut query, sort, columns, default_order)?;
    query.push(" LIMIT ").push_bind(limit).push(" OFFSET ").push_bind(offset);
    let params = query
        .build_query_as::<FlexParam>()
        .fetch_all(&mut **tx)
        .await?;

    let mut count = QueryBuilder::<Postgres>::new("SELECT COUNT(*) FROM flex_params");
    push_flex_param_conditions(&mut count, &scope, filters, columns)?;
    let (total,): (i64,) = count.build_query_as().fetch_one(&mut **tx).await?;

    Ok((params, total))
}

#[allow(clippy::too_many_arguments)]
pub async fn create_flex_param(
    tx: &mut Transaction<'_, Postgres>,
    type_param: &str,
    value_param: &str,
    description: Option<&str>,
    user_id: Uuid,
    header_id: Option<Uuid>,
    photo_id: Option<Uuid>,
    photo_url: Option<&str>,
    is_active: bool,
    created_by: &str,
) -> Result<FlexParam, AppError> {
    let param = sqlx::query_as::<_, FlexParam>(
        r#"
        INSERT INTO flex_params
            (type_param, value_param, description, user_id, header_id, photo_id, photo_url, is_active, created_by, updated_by)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $9)
        RETURNING *
        "#,
    )
    .bind(type_param)
    .bind(value_param)
    .bind(description)
    .bind(user_id)
    .bind(header_id)
    .bind(photo_id)
    .bind(photo_url)
    .bind(is_active)
    .bind(created_by)
    .fetch_one(&mut **tx)
    .await?;

    Ok(param)
}

#[allow(clippy::too_many_arguments)]
pub async fn update_flex_param(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    type_param: &str,
    value_param: &str,
    description: Option<&str>,
    header_id: Option<Uuid>,
    photo_id: Option<Uuid>,
    photo_url: Option<&str>,
    is_active: bool,
    updated_by: &str,
) -> Result<FlexParam, AppError> {
    let param = sqlx::query_as::<_, FlexParam>(
        r#"
        UPDATE flex_params
        SET type_param = $1, value_param = $2, description = $3, header_id = $4,
            photo_id = $5, photo_url = $6, is_active = $7, updated_by = $8, updated_at = now()
        WHERE id = $9 AND deleted_at IS NULL
        RETURNING *
        "#,
    )
    .bind(type_param)
    .bind(value_param)
    .bind(description)
    .bind(header_id)
    .bind(photo_id)
    .bind(photo_url)
    .bind(is_active)
    .bind(updated_by)
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| AppError::NotFound("Flex param tidak ditemukan".to_string()))?;

    Ok(param)
}

pub async fn soft_delete_flex_param(
    tx: &mut Transaction<'_, Postgres>,
    id: Uuid,
    deleted_by: &str,
) -> Result<(), AppError> {
    let result = sqlx::query(
        "UPDATE flex_params SET deleted_by = $1, deleted_at = now() WHERE id = $2 AND deleted_at IS NULL",
    )
    .bind(deleted_by)
    .bind(id)
    .execute(&mut **tx)
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::NotFound("Flex param tidak ditemukan".to_string()));
    }

    Ok(())
}
