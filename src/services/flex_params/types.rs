use serde::Deserialize;
use uuid::Uuid;
use validator::Validate;

#[derive(Debug, Deserialize, Validate)]
pub struct CreateFlexParamInput {
    #[validate(length(min = 1, message = "Type param wajib diisi"))]
    pub type_param: String,

    #[validate(length(min = 1, message = "Value param wajib diisi"))]
    pub value_param: String,

    pub description: Option<String>,
    pub header_id: Option<Uuid>,
    pub is_active: bool,
}

/// Query param endpoint list: filter & sort dinamis (format sama dengan backend Express) +
/// pagination. Contoh:
/// `?filter=[{"key":"description","operator":"in","value":["Large","Mid"]}]&sort=value_param&order=asc&page=1&limit=20`
#[derive(Debug, Default, Deserialize)]
pub struct ListQueryOptions {
    /// JSON array `[{"key","operator","value"}]` — operator: equal, notEqual, like, in, gt, gte, lt, lte.
    pub filter: Option<String>,
    /// Nama kolom untuk pengurutan.
    pub sort: Option<String>,
    /// `asc` (default) | `desc`.
    pub order: Option<String>,
    pub page: Option<i64>,
    pub limit: Option<i64>,
}

#[derive(Debug, Default, Deserialize, Validate)]
pub struct UpdateFlexParamInput {
    pub type_param: Option<String>,
    pub value_param: Option<String>,
    pub description: Option<String>,
    pub header_id: Option<Uuid>,
    pub is_active: Option<bool>,
}
