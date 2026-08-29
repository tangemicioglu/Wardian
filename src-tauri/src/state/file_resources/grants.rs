/// Short-lived one-shot authority returned by the native Save As picker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SaveTargetGrantV1 {
    /// Response schema version.
    pub schema: u8,
    /// Opaque backend-owned grant identifier.
    pub save_target_grant_id: String,
    /// Selected path for display only; it is not filesystem authority.
    pub selected_path: String,
}

/// Ordinary exact-file capability created by a successful Save As operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct FileResourceSaveAsResultV1 {
    /// Response schema version.
    pub schema: u8,
    /// Opaque exact-file capability identifier for later opening.
    pub capability_id: String,
    /// Verified canonical path of the saved ordinary file.
    pub canonical_path: String,
    /// Stable `file:` resource identifier derived from the canonical path.
    pub resource_id: String,
    /// Hash of the durably written content.
    pub content_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct UserFileGrantV1 {
    pub schema: u8,
    pub capability_id: String,
    pub canonical_path: String,
}

#[derive(Clone)]
struct UserFileGrant {
    canonical_path: String,
    authorized: AuthorizedPath,
    last_used_at: Instant,
    in_flight_uses: usize,
    active_subscriptions: usize,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DurableUserFileGrantStoreV1 {
    schema_version: u32,
    canonical_paths: Vec<String>,
}

struct UserFileGrantReservation {
    grants: OwnedMutexGuard<HashMap<String, UserFileGrant>>,
    capability_id: String,
    evict_capability_id: Option<String>,
    canonical_path: String,
}

impl UserFileGrantReservation {
    fn publish(mut self, authorized: AuthorizedPath) -> String {
        let now = Instant::now();
        if let Some(existing) = self.grants.get_mut(&self.capability_id) {
            existing.authorized = authorized;
            existing.last_used_at = now;
            return self.capability_id.clone();
        }
        if let Some(evict_capability_id) = &self.evict_capability_id {
            self.grants.remove(evict_capability_id);
        }
        self.grants.insert(
            self.capability_id.clone(),
            UserFileGrant {
                canonical_path: self.canonical_path,
                authorized,
                last_used_at: now,
                in_flight_uses: 0,
                active_subscriptions: 0,
            },
        );
        self.capability_id.clone()
    }
}

struct SaveTargetGrant {
    selected_path: PathBuf,
    requested_parent: PathBuf,
    canonical_parent: PathBuf,
    basename: OsString,
    parent: File,
    parent_identity: FilesystemIdentity,
    binding: SaveTargetBinding,
    expires_at: Instant,
}

enum SaveTargetBinding {
    Missing,
    Existing {
        authorized: AuthorizedPath,
        snapshot: Box<VerifiedFileSnapshot>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FilesystemIdentity {
    volume: u64,
    file: u64,
}

fn default_user_file_grant_store_path() -> Option<PathBuf> {
    crate::utils::fs::get_wardian_home().map(|home| home.join("settings").join("file-grants.json"))
}

fn load_durable_user_file_grants(
    store_path: &Path,
) -> Result<DurableUserFileGrantStoreV1, FileResourceErrorV1> {
    let bytes = match std::fs::read(store_path) {
        Ok(bytes) => bytes,
        Err(cause) if cause.kind() == std::io::ErrorKind::NotFound => {
            return Ok(DurableUserFileGrantStoreV1 {
                schema_version: USER_FILE_GRANT_STORE_SCHEMA_VERSION,
                canonical_paths: Vec::new(),
            });
        }
        Err(cause) => {
            return Err(error(
                "grant_store_unavailable",
                format!("cannot read exact file grants: {cause}"),
            ));
        }
    };
    let store = serde_json::from_slice::<DurableUserFileGrantStoreV1>(&bytes).map_err(|cause| {
        error(
            "grant_store_unavailable",
            format!("exact file grants are malformed: {cause}"),
        )
    })?;
    if store.schema_version != USER_FILE_GRANT_STORE_SCHEMA_VERSION
        || store
            .canonical_paths
            .iter()
            .any(|path| path.trim().is_empty())
    {
        return Err(error(
            "grant_store_unavailable",
            "exact file grants use an unsupported or invalid schema",
        ));
    }
    Ok(store)
}

fn durable_user_file_grant_matches(
    store_path: &Path,
    canonical_path: &str,
) -> Result<bool, FileResourceErrorV1> {
    Ok(load_durable_user_file_grants(store_path)?
        .canonical_paths
        .iter()
        .any(|granted| granted == canonical_path))
}

fn upsert_durable_user_file_grant(
    store_path: &Path,
    canonical_path: &str,
    max_grants: usize,
) -> Result<(), FileResourceErrorV1> {
    let mut store = load_durable_user_file_grants(store_path)?;
    store
        .canonical_paths
        .retain(|granted| granted != canonical_path);
    store.canonical_paths.push(canonical_path.to_string());
    if store.canonical_paths.len() > max_grants {
        let overflow = store.canonical_paths.len() - max_grants;
        store.canonical_paths.drain(..overflow);
    }
    wardian_core::conversations::write_json_atomic(store_path, &store).map_err(|cause| {
        error(
            "grant_store_unavailable",
            format!("cannot persist exact file grants: {cause}"),
        )
    })
}
