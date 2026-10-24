use sqlx::{Postgres, QueryBuilder, Transaction};
use uuid::Uuid;

use super::model::UserScreenerRule;
use super::types::NewScreenerRule;
use crate::utils::app_error::AppError;

/// Rule beberapa strategi sekaligus, urut `sort_order`.
pub async fn find_rules_of_screeners(
    tx: &mut Transaction<'_, Postgres>,
    screener_ids: &[Uuid],
) -> Result<Vec<UserScreenerRule>, AppError> {
    let rules = sqlx::query_as::<_, UserScreenerRule>(
        r#"
        SELECT * FROM user_screener_rules
        WHERE screener_id = ANY($1) AND deleted_at IS NULL
        ORDER BY screener_id, sort_order
        "#,
    )
    .bind(screener_ids)
    .fetch_all(&mut **tx)
    .await?;
    Ok(rules)
}

/// Simpan rule baru 1 strategi (1 query untuk semua rule).
pub async fn create_rules(
    tx: &mut Transaction<'_, Postgres>,
    screener_id: Uuid,
    rules: &[NewScreenerRule],
    actor: &str,
) -> Result<Vec<UserScreenerRule>, AppError> {
    if rules.is_empty() {
        return Ok(Vec::new());
    }
    let mut query = QueryBuilder::<Postgres>::new(
        "INSERT INTO user_screener_rules \
         (screener_id, flex_param_id, indicator_code, params, condition, weight, sort_order, created_by, updated_by) ",
    );
    query.push_values(rules, |mut row, rule| {
        row.push_bind(screener_id)
            .push_bind(rule.flex_param_id)
            .push_bind(&rule.indicator_code)
            .push_bind(&rule.params)
            .push_bind(&rule.condition)
            .push_bind(rule.weight)
            .push_bind(rule.sort_order)
            .push_bind(actor)
            .push_bind(actor);
    });
    query.push(" RETURNING *");
    let mut created: Vec<UserScreenerRule> = query.build_query_as().fetch_all(&mut **tx).await?;
    created.sort_by_key(|r| r.sort_order);
    Ok(created)
}

/// Soft delete semua rule 1 strategi — dipakai saat strategi diganti isinya / dihapus. Rule lama
/// tetap ada di DB (bisa dirujuk histori transaksi).
pub async fn soft_delete_rules_of_screener(
    tx: &mut Transaction<'_, Postgres>,
    screener_id: Uuid,
    actor: &str,
) -> Result<(), AppError> {
    sqlx::query(
        r#"
        UPDATE user_screener_rules SET deleted_at = now(), deleted_by = $2, updated_at = now(), updated_by = $2
        WHERE screener_id = $1 AND deleted_at IS NULL
        "#,
    )
    .bind(screener_id)
    .bind(actor)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
