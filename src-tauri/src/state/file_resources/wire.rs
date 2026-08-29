#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct FileResourceSnapshotV1 {
    pub resource_id: String,
    pub subscription_id: String,
    pub revision: u64,
    pub descriptor: FileContentDescriptorV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct FileResourceEventV1 {
    pub schema: u8,
    pub resource_id: String,
    pub revision: u64,
    pub descriptor: FileContentDescriptorV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct FileResourceTextV1 {
    pub schema: u8,
    pub resource_id: String,
    pub revision: u64,
    pub text: String,
}

/// Tagged optimistic-save result returned to the frontend without exposing the
/// backend-private retained-handle revision token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum FileResourceSaveResultV1 {
    /// The submitted text replaced the target and advanced the revision.
    Saved { revision: u64, content_hash: String },
    /// The submitted text was byte-identical to the current target.
    Unchanged { revision: u64, content_hash: String },
    /// The editor base no longer matches the currently authorized target.
    StaleConflict { revision: u64, content_hash: String },
}
