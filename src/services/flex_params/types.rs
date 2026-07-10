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

#[derive(Debug, Default, Deserialize, Validate)]
pub struct UpdateFlexParamInput {
    pub type_param: Option<String>,
    pub value_param: Option<String>,
    pub description: Option<String>,
    pub header_id: Option<Uuid>,
    pub is_active: Option<bool>,
}
