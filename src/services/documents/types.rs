use uuid::Uuid;

/// Metadata yang menyertai upload/update file. `ref_id`/`ref_type` dipakai untuk
/// mengaitkan file ke entitas lain (mis. ref_type="USER_PROFILE", ref_id=user_id).
#[derive(Debug, Default)]
pub struct FileMetaInput {
    pub ref_id: Option<Uuid>,
    pub ref_type: Option<String>,
}
