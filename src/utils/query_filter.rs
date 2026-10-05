//! Filter & sort dinamis untuk endpoint list, format sama dengan backend Express (luma):
//!
//! - `?filter=[{"key":"type_param","operator":"equal","value":"SIMBOL_CRYPTO"}]` — JSON array,
//!   semua kondisi digabung dengan AND (kolom yang sama boleh dipakai lebih dari sekali, mis.
//!   rentang tanggal `created_at gte ...` + `created_at lte ...`).
//! - `?sort=created_at&order=desc` — 1 kolom, `order` default `asc`.
//!
//! Nama kolom hanya bisa berasal dari whitelist (`FilterColumn`) milik tiap endpoint — input user
//! tidak pernah masuk ke SQL sebagai nama kolom, dan semua nilai dikirim sebagai parameter (bind).

use std::str::FromStr;

use rust_decimal::Decimal;
use serde::Deserialize;
use serde_json::Value;
use sqlx::{Postgres, QueryBuilder};
use uuid::Uuid;

use crate::utils::app_error::AppError;

/// Tipe kolom — menentukan validasi nilai filter & cast di SQL.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ColumnKind {
    Text,
    Uuid,
    Bool,
    Integer,
    Numeric,
    /// Terima `YYYY-MM-DD` atau RFC 3339 (`2026-09-01T07:00:00+07:00`).
    Timestamp,
}

impl ColumnKind {
    fn sql_type(self) -> &'static str {
        match self {
            ColumnKind::Text => "TEXT",
            ColumnKind::Uuid => "UUID",
            ColumnKind::Bool => "BOOLEAN",
            ColumnKind::Integer => "BIGINT",
            ColumnKind::Numeric => "NUMERIC",
            ColumnKind::Timestamp => "TIMESTAMPTZ",
        }
    }
}

/// 1 kolom yang boleh difilter/diurutkan di sebuah endpoint.
#[derive(Debug, Clone, Copy)]
pub struct FilterColumn {
    pub name: &'static str,
    pub kind: ColumnKind,
}

impl FilterColumn {
    pub const fn new(name: &'static str, kind: ColumnKind) -> Self {
        Self { name, kind }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum FilterOperator {
    Equal,
    NotEqual,
    Like,
    In,
    Gt,
    Gte,
    Lt,
    Lte,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FilterCondition {
    pub key: String,
    pub operator: FilterOperator,
    #[serde(default)]
    pub value: Value,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SortOrder {
    Asc,
    Desc,
}

#[derive(Debug, Clone)]
pub struct SortCondition {
    pub key: String,
    pub order: SortOrder,
}

/// Parse `?filter=` (JSON array). Kosong = tanpa filter.
pub fn parse_filter_query(raw: Option<&str>) -> Result<Vec<FilterCondition>, AppError> {
    let Some(raw) = raw.map(str::trim).filter(|r| !r.is_empty()) else {
        return Ok(Vec::new());
    };

    let parsed: Value = serde_json::from_str(raw)
        .map_err(|_| AppError::BadRequest("filter harus berupa JSON array yang valid".to_string()))?;
    if !parsed.is_array() {
        return Err(AppError::BadRequest("filter harus berupa JSON array".to_string()));
    }

    serde_json::from_value(parsed).map_err(|_| {
        AppError::BadRequest(
            "filter tidak valid, wajib berisi key dan operator yang didukung \
             (equal, notEqual, like, in, gt, gte, lt, lte)"
                .to_string(),
        )
    })
}

/// Parse `?sort=<kolom>&order=asc|desc`. Kosong = pakai urutan default endpoint.
pub fn parse_sort_query(sort: Option<&str>, order: Option<&str>) -> Result<Option<SortCondition>, AppError> {
    let Some(sort) = sort.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };

    let order = match order.map(|o| o.trim().to_lowercase()).as_deref() {
        None | Some("") | Some("asc") => SortOrder::Asc,
        Some("desc") => SortOrder::Desc,
        Some(_) => return Err(AppError::BadRequest("order harus 'asc' atau 'desc'".to_string())),
    };

    Ok(Some(SortCondition {
        key: sort.to_string(),
        order,
    }))
}

fn find_column(key: &str, columns: &[FilterColumn]) -> Result<FilterColumn, AppError> {
    columns
        .iter()
        .copied()
        .find(|c| c.name == key)
        .ok_or_else(|| AppError::BadRequest(format!("Kolom '{key}' tidak dapat difilter/diurutkan")))
}

/// Nilai JSON -> teks yang aman di-cast ke tipe kolom. Divalidasi di sini supaya nilai yang salah
/// format jadi 400 Bad Request, bukan error database.
fn value_to_text(value: &Value, column: FilterColumn) -> Result<String, AppError> {
    let invalid = || {
        AppError::BadRequest(format!(
            "Nilai filter '{}' tidak sesuai tipe kolom",
            column.name
        ))
    };
    let text = match value {
        Value::String(s) => s.trim().to_string(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => return Err(invalid()),
    };

    let valid = match column.kind {
        ColumnKind::Text => true,
        ColumnKind::Uuid => Uuid::parse_str(&text).is_ok(),
        ColumnKind::Bool => matches!(text.as_str(), "true" | "false"),
        ColumnKind::Integer => text.parse::<i64>().is_ok(),
        ColumnKind::Numeric => Decimal::from_str(&text).is_ok(),
        ColumnKind::Timestamp => {
            chrono::DateTime::parse_from_rfc3339(&text).is_ok()
                || chrono::NaiveDate::parse_from_str(&text, "%Y-%m-%d").is_ok()
        }
    };
    if valid { Ok(text) } else { Err(invalid()) }
}

/// Tambahkan kondisi filter ke query yang SUDAH punya `WHERE` (tiap kondisi ditambah sebagai
/// `AND ...`). `value: null` dengan `equal`/`notEqual` jadi `IS NULL`/`IS NOT NULL`.
pub fn push_filters(
    builder: &mut QueryBuilder<'_, Postgres>,
    filters: &[FilterCondition],
    columns: &[FilterColumn],
) -> Result<(), AppError> {
    for filter in filters {
        let column = find_column(&filter.key, columns)?;
        let sql_type = column.kind.sql_type();
        builder.push(" AND ").push(column.name);

        match (filter.operator, &filter.value) {
            (FilterOperator::Equal, Value::Null) => {
                builder.push(" IS NULL");
            }
            (FilterOperator::NotEqual, Value::Null) => {
                builder.push(" IS NOT NULL");
            }
            (FilterOperator::Like, value) => {
                if column.kind != ColumnKind::Text {
                    return Err(AppError::BadRequest(format!(
                        "Operator like hanya untuk kolom teks, '{}' bukan teks",
                        column.name
                    )));
                }
                // ILIKE: tidak membedakan huruf besar/kecil ("btc" cocok dengan "BTCUSDT").
                builder
                    .push(" ILIKE '%' || ")
                    .push_bind(value_to_text(value, column)?)
                    .push(" || '%'");
            }
            (FilterOperator::In, value) => {
                let items = match value {
                    Value::Array(items) => items.iter().collect::<Vec<_>>(),
                    other => vec![other],
                };
                if items.is_empty() {
                    return Err(AppError::BadRequest(format!(
                        "Nilai filter in untuk '{}' tidak boleh kosong",
                        column.name
                    )));
                }
                let values = items
                    .into_iter()
                    .map(|v| value_to_text(v, column))
                    .collect::<Result<Vec<_>, _>>()?;
                builder
                    .push(" = ANY(")
                    .push_bind(values)
                    .push(format!("::{sql_type}[])"));
            }
            (operator, value) => {
                let symbol = match operator {
                    FilterOperator::Equal => " = ",
                    FilterOperator::NotEqual => " <> ",
                    FilterOperator::Gt => " > ",
                    FilterOperator::Gte => " >= ",
                    FilterOperator::Lt => " < ",
                    FilterOperator::Lte => " <= ",
                    FilterOperator::Like | FilterOperator::In => unreachable!("ditangani di atas"),
                };
                builder
                    .push(symbol)
                    .push_bind(value_to_text(value, column)?)
                    .push(format!("::{sql_type}"));
            }
        }
    }
    Ok(())
}

/// Tambahkan `ORDER BY`. Tanpa sort dari user -> `default_order` (SQL tetap dari kode, mis.
/// `"created_at DESC"`).
pub fn push_order_by(
    builder: &mut QueryBuilder<'_, Postgres>,
    sort: Option<&SortCondition>,
    columns: &[FilterColumn],
    default_order: &str,
) -> Result<(), AppError> {
    builder.push(" ORDER BY ");
    match sort {
        Some(sort) => {
            let column = find_column(&sort.key, columns)?;
            let direction = match sort.order {
                SortOrder::Asc => "ASC",
                SortOrder::Desc => "DESC",
            };
            builder.push(column.name).push(" ").push(direction);
        }
        None => {
            builder.push(default_order);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const COLUMNS: [FilterColumn; 4] = [
        FilterColumn::new("value_param", ColumnKind::Text),
        FilterColumn::new("description", ColumnKind::Text),
        FilterColumn::new("is_active", ColumnKind::Bool),
        FilterColumn::new("created_at", ColumnKind::Timestamp),
    ];

    fn build(filter: &str, sort: Option<&str>, order: Option<&str>) -> Result<String, AppError> {
        let filters = parse_filter_query(Some(filter))?;
        let sort = parse_sort_query(sort, order)?;
        let mut query = QueryBuilder::<Postgres>::new("SELECT * FROM t WHERE deleted_at IS NULL");
        push_filters(&mut query, &filters, &COLUMNS)?;
        push_order_by(&mut query, sort.as_ref(), &COLUMNS, "created_at DESC")?;
        Ok(query.sql().to_string())
    }

    #[test]
    fn semua_operator_jadi_sql_dengan_bind() {
        let sql = build(
            r#"[{"key":"value_param","operator":"like","value":"btc"},
                {"key":"description","operator":"in","value":["Large","Mid"]},
                {"key":"is_active","operator":"equal","value":true},
                {"key":"created_at","operator":"gte","value":"2026-09-01"},
                {"key":"created_at","operator":"lte","value":"2026-09-30"},
                {"key":"description","operator":"notEqual","value":null}]"#,
            Some("value_param"),
            Some("desc"),
        )
        .unwrap();
        assert_eq!(
            sql,
            "SELECT * FROM t WHERE deleted_at IS NULL \
             AND value_param ILIKE '%' || $1 || '%' \
             AND description = ANY($2::TEXT[]) \
             AND is_active = $3::BOOLEAN \
             AND created_at >= $4::TIMESTAMPTZ \
             AND created_at <= $5::TIMESTAMPTZ \
             AND description IS NOT NULL \
             ORDER BY value_param DESC"
        );
    }

    #[test]
    fn tanpa_sort_pakai_default() {
        let sql = build("[]", None, None).unwrap();
        assert_eq!(sql, "SELECT * FROM t WHERE deleted_at IS NULL ORDER BY created_at DESC");
    }

    #[test]
    fn kolom_di_luar_whitelist_ditolak() {
        assert!(build(r#"[{"key":"password","operator":"equal","value":"x"}]"#, None, None).is_err());
        assert!(build("[]", Some("password"), None).is_err());
        // percobaan injeksi lewat nama kolom tidak pernah sampai ke SQL
        assert!(build("[]", Some("value_param; DROP TABLE t"), None).is_err());
    }

    #[test]
    fn input_tidak_valid_ditolak() {
        assert!(build("bukan json", None, None).is_err());
        assert!(build(r#"{"key":"value_param"}"#, None, None).is_err());
        assert!(build(r#"[{"key":"value_param","operator":"regex","value":"x"}]"#, None, None).is_err());
        assert!(build(r#"[{"key":"is_active","operator":"equal","value":"ya"}]"#, None, None).is_err());
        assert!(build(r#"[{"key":"created_at","operator":"gte","value":"kemarin"}]"#, None, None).is_err());
        assert!(build(r#"[{"key":"is_active","operator":"like","value":"t"}]"#, None, None).is_err());
        assert!(build("[]", Some("value_param"), Some("naik")).is_err());
    }
}
