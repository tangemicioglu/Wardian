/// Metadata returned after an editor recovery checkpoint is durably committed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct FileRecoveryCheckpointV1 {
    pub schema: u8,
    pub recovery_id: String,
    pub resource_key: String,
    pub base_content_hash: String,
    pub base_opaque_revision: String,
    pub recovery_revision: u64,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    /// Advisory current-file authorization failure observed after the recovery
    /// bytes committed. This never gates or rolls back recovery durability.
    pub file_authorization_error: Option<FileResourceErrorV1>,
}

/// Body-free metadata used to discover durable editor recovery records after
/// a frontend or native runtime restart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct FileRecoverySummaryV1 {
    pub schema: u8,
    pub recovery_id: String,
    pub resource_key: String,
    pub display_name: String,
    pub extension: Option<String>,
    pub mime_type: String,
    pub base_content_hash: String,
    pub base_opaque_revision: String,
    pub recovery_revision: u64,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

/// Optional exact durable-recovery generation cleaned after a successful
/// guarded save. The calling WebView scope is supplied by the command layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct FileRecoveryCleanupV1 {
    pub recovery_id: String,
    pub expected_recovery_revision: u64,
}

/// Read-only durable editor recovery payload. It contains only the persisted
/// base and buffer; current filesystem bytes require a live subscription.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct FileRecoveryV1 {
    pub schema: u8,
    pub recovery_id: String,
    pub resource_key: String,
    pub display_name: String,
    pub extension: Option<String>,
    pub mime_type: String,
    pub base_content_hash: String,
    pub base_opaque_revision: String,
    pub recovery_revision: u64,
    pub base: String,
    pub buffer: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

/// Structured three-way recovery merge outcome. Conflicts always return
/// explicit markers instead of selecting either editor or disk bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum FileRecoveryMergeResultV1 {
    Clean {
        recovery_revision: u64,
        current_revision: u64,
        current_content_hash: String,
        disk_changed: bool,
        merged_text: String,
    },
    Conflicted {
        recovery_revision: u64,
        current_revision: u64,
        current_content_hash: String,
        disk_changed: bool,
        merged_text: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
struct FileRecoveryManifestV1 {
    schema: u8,
    recovery_id: String,
    resource_key: String,
    display_name: String,
    extension: Option<String>,
    mime_type: String,
    base_content_hash: String,
    base_opaque_revision: String,
    base_blob: String,
    buffer_blob: String,
    recovery_revision: u64,
    webview_scope: String,
    created_at_ms: u64,
    updated_at_ms: u64,
}

#[derive(Debug, Clone, Copy)]
struct FileRecoveryStoreLimits {
    max_records: usize,
    max_body_bytes: u64,
    orphan_grace_period: Duration,
}

impl Default for FileRecoveryStoreLimits {
    fn default() -> Self {
        Self {
            max_records: DEFAULT_MAX_RECOVERY_RECORDS,
            max_body_bytes: DEFAULT_MAX_RECOVERY_BODY_BYTES,
            orphan_grace_period: RECOVERY_ORPHAN_GRACE_PERIOD,
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
struct FileRecoveryStoreUsage {
    records: usize,
    body_bytes: u64,
}

fn default_recovery_root() -> Option<PathBuf> {
    crate::utils::fs::get_wardian_home().map(|home| home.join("files").join("recovery"))
}

fn recovery_record_ids(recovery_root: &Path) -> Result<Vec<String>, FileResourceErrorV1> {
    match std::fs::symlink_metadata(recovery_root) {
        Ok(_) => validate_recovery_root(recovery_root)?,
        Err(cause) if cause.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(cause) => {
            return Err(error(
                "recovery_unavailable",
                format!("cannot inspect recovery root: {cause}"),
            ));
        }
    }
    let entries = std::fs::read_dir(recovery_root).map_err(|cause| {
        error(
            "recovery_unavailable",
            format!("cannot enumerate recovery records: {cause}"),
        )
    })?;
    let mut recovery_ids = Vec::new();
    for entry in entries.flatten() {
        let Some(recovery_id) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let Ok(parsed) = Uuid::parse_str(&recovery_id) else {
            continue;
        };
        if parsed.to_string() != recovery_id {
            continue;
        }
        let Ok(metadata) = entry.path().symlink_metadata() else {
            continue;
        };
        if metadata.file_type().is_dir()
            && validate_direct_child_directory(recovery_root, &entry.path(), "record directory")
                .is_ok()
        {
            recovery_ids.push(recovery_id);
        }
    }
    recovery_ids.sort();
    Ok(recovery_ids)
}

fn recovery_record_dir(
    recovery_root: &Path,
    recovery_id: &str,
) -> Result<PathBuf, FileResourceErrorV1> {
    let parsed = Uuid::parse_str(recovery_id).map_err(|_| {
        error(
            "invalid_request",
            "recovery id is not a valid opaque identifier",
        )
    })?;
    if parsed.to_string() != recovery_id {
        return Err(error(
            "invalid_request",
            "recovery id is not in canonical form",
        ));
    }
    Ok(recovery_root.join(recovery_id))
}

fn validate_direct_child_directory(
    parent: &Path,
    child: &Path,
    label: &str,
) -> Result<(), FileResourceErrorV1> {
    let parent_metadata = std::fs::symlink_metadata(parent).map_err(|cause| {
        error(
            "recovery_unavailable",
            format!("cannot inspect recovery {label} parent: {cause}"),
        )
    })?;
    let child_metadata = std::fs::symlink_metadata(child).map_err(|cause| {
        error(
            "recovery_unavailable",
            format!("cannot inspect recovery {label}: {cause}"),
        )
    })?;
    if !parent_metadata.file_type().is_dir() || !child_metadata.file_type().is_dir() {
        return Err(error(
            "invalid_recovery",
            format!("recovery {label} is not an ordinary directory"),
        ));
    }
    let canonical_parent = std::fs::canonicalize(parent).map_err(|cause| {
        error(
            "recovery_unavailable",
            format!("cannot resolve recovery {label} parent: {cause}"),
        )
    })?;
    let canonical_child = std::fs::canonicalize(child).map_err(|cause| {
        error(
            "recovery_unavailable",
            format!("cannot resolve recovery {label}: {cause}"),
        )
    })?;
    if canonical_child.parent() != Some(canonical_parent.as_path()) {
        return Err(error(
            "invalid_recovery",
            format!("recovery {label} is not a direct child of its backend-owned parent"),
        ));
    }
    Ok(())
}

fn validate_recovery_root(recovery_root: &Path) -> Result<(), FileResourceErrorV1> {
    let metadata = std::fs::symlink_metadata(recovery_root).map_err(|cause| {
        error(
            "recovery_unavailable",
            format!("cannot inspect recovery root: {cause}"),
        )
    })?;
    if !metadata.file_type().is_dir() {
        return Err(error(
            "invalid_recovery",
            "recovery root is not an ordinary directory",
        ));
    }
    Ok(())
}

fn validate_recovery_record_dir(
    recovery_root: &Path,
    recovery_id: &str,
) -> Result<PathBuf, FileResourceErrorV1> {
    let record_dir = recovery_record_dir(recovery_root, recovery_id)?;
    match std::fs::symlink_metadata(&record_dir) {
        Ok(_) => {}
        Err(cause) if cause.kind() == std::io::ErrorKind::NotFound => {
            return Err(error(
                "recovery_not_found",
                "recovery checkpoint does not exist",
            ));
        }
        Err(cause) => {
            return Err(error(
                "recovery_unavailable",
                format!("cannot inspect recovery checkpoint: {cause}"),
            ));
        }
    }
    validate_direct_child_directory(recovery_root, &record_dir, "record directory")?;
    Ok(record_dir)
}

fn recovery_checkpoint_metadata(
    manifest: &FileRecoveryManifestV1,
    file_authorization_error: Option<FileResourceErrorV1>,
) -> FileRecoveryCheckpointV1 {
    FileRecoveryCheckpointV1 {
        schema: 1,
        recovery_id: manifest.recovery_id.clone(),
        resource_key: manifest.resource_key.clone(),
        base_content_hash: manifest.base_content_hash.clone(),
        base_opaque_revision: manifest.base_opaque_revision.clone(),
        recovery_revision: manifest.recovery_revision,
        created_at_ms: manifest.created_at_ms,
        updated_at_ms: manifest.updated_at_ms,
        file_authorization_error,
    }
}

fn recovery_summary_metadata(manifest: &FileRecoveryManifestV1) -> FileRecoverySummaryV1 {
    FileRecoverySummaryV1 {
        schema: 1,
        recovery_id: manifest.recovery_id.clone(),
        resource_key: manifest.resource_key.clone(),
        display_name: manifest.display_name.clone(),
        extension: manifest.extension.clone(),
        mime_type: manifest.mime_type.clone(),
        base_content_hash: manifest.base_content_hash.clone(),
        base_opaque_revision: manifest.base_opaque_revision.clone(),
        recovery_revision: manifest.recovery_revision,
        created_at_ms: manifest.created_at_ms,
        updated_at_ms: manifest.updated_at_ms,
    }
}

fn authorize_recovery_manifest(
    manifest: &FileRecoveryManifestV1,
    resource_key: &str,
    webview_scope: &str,
) -> Result<(), FileResourceErrorV1> {
    if manifest.schema != 1
        || manifest.resource_key != resource_key
        || manifest.webview_scope != webview_scope
    {
        return Err(error(
            "unauthorized_recovery",
            "recovery does not belong to this resource and webview scope",
        ));
    }
    Ok(())
}

fn load_recovery_manifest(
    recovery_root: &Path,
    recovery_id: &str,
) -> Result<FileRecoveryManifestV1, FileResourceErrorV1> {
    let record_dir = validate_recovery_record_dir(recovery_root, recovery_id)?;
    let path = record_dir.join("manifest.json");
    let metadata = std::fs::symlink_metadata(&path).map_err(|cause| {
        if cause.kind() == std::io::ErrorKind::NotFound {
            error("recovery_not_found", "recovery checkpoint does not exist")
        } else {
            error(
                "recovery_unavailable",
                format!("cannot inspect recovery manifest: {cause}"),
            )
        }
    })?;
    if !metadata.file_type().is_file() || metadata.len() > 64 * 1024 {
        return Err(error(
            "invalid_recovery",
            "recovery manifest is not a bounded ordinary file",
        ));
    }
    let bytes = std::fs::read(&path).map_err(|cause| {
        error(
            "recovery_unavailable",
            format!("cannot read recovery manifest: {cause}"),
        )
    })?;
    let manifest: FileRecoveryManifestV1 = serde_json::from_slice(&bytes).map_err(|cause| {
        error(
            "invalid_recovery",
            format!("recovery manifest is invalid: {cause}"),
        )
    })?;
    if manifest.recovery_id != recovery_id || manifest.schema != 1 {
        return Err(error(
            "invalid_recovery",
            "recovery manifest identity or schema is invalid",
        ));
    }
    Ok(manifest)
}

fn recovery_blob_name(text: &str) -> String {
    format!("sha256-{:x}.txt", Sha256::digest(text.as_bytes()))
}

fn is_recovery_blob_name(blob_name: &str) -> bool {
    blob_name.starts_with("sha256-")
        && blob_name.ends_with(".txt")
        && blob_name.len() == "sha256-".len() + 64 + ".txt".len()
        && blob_name["sha256-".len()..blob_name.len() - ".txt".len()]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
}

fn write_recovery_blob(record_dir: &Path, text: &str) -> Result<String, FileResourceErrorV1> {
    let blob_name = recovery_blob_name(text);
    let blobs_dir = record_dir.join("blobs");
    std::fs::create_dir_all(&blobs_dir).map_err(|cause| {
        error(
            "recovery_unavailable",
            format!("cannot create recovery blob directory: {cause}"),
        )
    })?;
    validate_direct_child_directory(record_dir, &blobs_dir, "blob directory")?;
    let path = blobs_dir.join(&blob_name);
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) => {
            validate_existing_recovery_blob(&path, &metadata, text)?;
            return Ok(blob_name);
        }
        Err(cause) if cause.kind() == std::io::ErrorKind::NotFound => {}
        Err(cause) => {
            return Err(error(
                "invalid_recovery",
                format!("cannot inspect recovery blob target: {cause}"),
            ));
        }
    }
    match OpenOptions::new().create_new(true).write(true).open(&path) {
        Ok(mut file) => {
            file.write_all(text.as_bytes())
                .and_then(|()| file.sync_all())
                .map_err(|cause| {
                    let _ = std::fs::remove_file(&path);
                    error(
                        "recovery_unavailable",
                        format!("cannot write recovery blob: {cause}"),
                    )
                })?;
            sync_recovery_directory(&blobs_dir).map_err(|cause| {
                error(
                    "recovery_unavailable",
                    format!("cannot flush recovery blob directory: {cause}"),
                )
            })?;
        }
        Err(cause) if cause.kind() == std::io::ErrorKind::AlreadyExists => {
            let metadata = std::fs::symlink_metadata(&path).map_err(|metadata_cause| {
                error(
                    "invalid_recovery",
                    format!("cannot inspect existing recovery blob: {metadata_cause}"),
                )
            })?;
            validate_existing_recovery_blob(&path, &metadata, text)?;
        }
        Err(cause) => {
            return Err(error(
                "recovery_unavailable",
                format!("cannot create recovery blob: {cause}"),
            ));
        }
    }
    Ok(blob_name)
}

fn validate_existing_recovery_blob(
    path: &Path,
    metadata: &std::fs::Metadata,
    text: &str,
) -> Result<(), FileResourceErrorV1> {
    let expected_length = u64::try_from(text.len()).map_err(|_| {
        error(
            "file_too_large",
            "recovery blob length cannot fit in the supported size",
        )
    })?;
    if !metadata.file_type().is_file() || metadata.len() != expected_length {
        return Err(error(
            "invalid_recovery",
            "existing recovery blob is not the expected bounded ordinary file",
        ));
    }
    let existing = std::fs::read(path).map_err(|cause| {
        error(
            "invalid_recovery",
            format!("cannot verify existing recovery blob: {cause}"),
        )
    })?;
    if existing != text.as_bytes() {
        return Err(error(
            "invalid_recovery",
            "hash-addressed recovery blob contains different bytes",
        ));
    }
    Ok(())
}

fn read_recovery_blob(
    recovery_root: &Path,
    manifest: &FileRecoveryManifestV1,
    blob_name: &str,
    limits: &FileResourceLimits,
) -> Result<String, FileResourceErrorV1> {
    let path = validate_recovery_blob_metadata(recovery_root, manifest, blob_name, limits)?;
    let bytes = std::fs::read(&path).map_err(|cause| {
        error(
            "invalid_recovery",
            format!("cannot read recovery blob: {cause}"),
        )
    })?;
    let text = String::from_utf8(bytes)
        .map_err(|_| error("invalid_recovery", "recovery blob is not valid UTF-8 text"))?;
    if recovery_blob_name(&text) != blob_name {
        return Err(error(
            "invalid_recovery",
            "recovery blob hash does not match its immutable name",
        ));
    }
    validate_submitted_text(&text, limits).map_err(|_| {
        error(
            "invalid_recovery",
            "recovery blob exceeds the complete text-model limits",
        )
    })?;
    Ok(text)
}

fn validate_recovery_blob_metadata(
    recovery_root: &Path,
    manifest: &FileRecoveryManifestV1,
    blob_name: &str,
    limits: &FileResourceLimits,
) -> Result<PathBuf, FileResourceErrorV1> {
    if !is_recovery_blob_name(blob_name) {
        return Err(error(
            "invalid_recovery",
            "recovery blob name is not hash-addressed",
        ));
    }
    let record_dir = validate_recovery_record_dir(recovery_root, &manifest.recovery_id)?;
    let blobs_dir = record_dir.join("blobs");
    validate_direct_child_directory(&record_dir, &blobs_dir, "blob directory")?;
    let path = blobs_dir.join(blob_name);
    let metadata = std::fs::symlink_metadata(&path).map_err(|cause| {
        error(
            "invalid_recovery",
            format!("recovery blob is unavailable: {cause}"),
        )
    })?;
    if !metadata.file_type().is_file() || metadata.len() > limits.monaco_max_size_bytes {
        return Err(error(
            "invalid_recovery",
            "recovery blob is not a bounded ordinary file",
        ));
    }
    Ok(path)
}

fn sweep_recovery_store(
    recovery_root: &Path,
    orphan_grace_period: Duration,
) -> Result<(), FileResourceErrorV1> {
    for recovery_id in recovery_record_ids(recovery_root)? {
        match load_recovery_manifest(recovery_root, &recovery_id) {
            Ok(manifest) => {
                garbage_collect_recovery_blobs(recovery_root, &manifest, orphan_grace_period)
            }
            Err(failure) if failure.code() == "recovery_not_found" => {
                let record_dir = validate_recovery_record_dir(recovery_root, &recovery_id)?;
                let metadata = std::fs::symlink_metadata(&record_dir).map_err(|cause| {
                    error(
                        "recovery_unavailable",
                        format!("cannot inspect incomplete recovery record: {cause}"),
                    )
                })?;
                let old_enough = metadata
                    .modified()
                    .ok()
                    .and_then(|modified| modified.elapsed().ok())
                    .is_some_and(|age| age >= orphan_grace_period);
                if old_enough {
                    remove_recovery_record(recovery_root, &recovery_id)?;
                }
            }
            Err(_) => {
                // Malformed or temporarily unreadable records remain intact.
                // Admission accounting below still charges their directories
                // and ordinary body files against the bounded store.
            }
        }
    }
    Ok(())
}

fn recovery_store_usage(
    recovery_root: &Path,
) -> Result<FileRecoveryStoreUsage, FileResourceErrorV1> {
    let recovery_ids = recovery_record_ids(recovery_root)?;
    let mut usage = FileRecoveryStoreUsage {
        records: recovery_ids.len(),
        body_bytes: 0,
    };
    for recovery_id in recovery_ids {
        let record_dir = recovery_record_dir(recovery_root, &recovery_id)?;
        let blobs_dir = record_dir.join("blobs");
        let metadata = match std::fs::symlink_metadata(&blobs_dir) {
            Ok(metadata) => metadata,
            Err(cause) if cause.kind() == std::io::ErrorKind::NotFound => continue,
            Err(cause) => {
                return Err(error(
                    "recovery_unavailable",
                    format!("cannot inspect recovery blob directory: {cause}"),
                ));
            }
        };
        if !metadata.file_type().is_dir()
            || validate_direct_child_directory(&record_dir, &blobs_dir, "blob directory").is_err()
        {
            continue;
        }
        let entries = std::fs::read_dir(&blobs_dir).map_err(|cause| {
            error(
                "recovery_unavailable",
                format!("cannot enumerate recovery blobs: {cause}"),
            )
        })?;
        for entry in entries.flatten() {
            let Ok(metadata) = entry.path().symlink_metadata() else {
                continue;
            };
            if metadata.file_type().is_file() {
                usage.body_bytes = usage.body_bytes.saturating_add(metadata.len());
            }
        }
    }
    Ok(usage)
}

fn enforce_recovery_admission(
    recovery_root: &Path,
    recovery_id: &str,
    base: &str,
    buffer: &str,
    limits: FileRecoveryStoreLimits,
) -> Result<(), FileResourceErrorV1> {
    let usage = recovery_store_usage(recovery_root)?;
    let record_dir = recovery_record_dir(recovery_root, recovery_id)?;
    let record_exists = match std::fs::symlink_metadata(&record_dir) {
        Ok(metadata) => {
            if !metadata.file_type().is_dir() {
                return Err(error(
                    "invalid_recovery",
                    "recovery record target is not an ordinary directory",
                ));
            }
            true
        }
        Err(cause) if cause.kind() == std::io::ErrorKind::NotFound => false,
        Err(cause) => {
            return Err(error(
                "recovery_unavailable",
                format!("cannot inspect recovery record admission target: {cause}"),
            ));
        }
    };
    if !record_exists && usage.records >= limits.max_records {
        return Err(error(
            "recovery_capacity_exceeded",
            "durable editor recovery record limit is reached",
        ));
    }

    let mut additional_bytes = 0_u64;
    let mut prospective_names = HashSet::new();
    for text in [base, buffer] {
        let blob_name = recovery_blob_name(text);
        if !prospective_names.insert(blob_name.clone()) {
            continue;
        }
        let already_exists = if record_exists {
            match std::fs::symlink_metadata(record_dir.join("blobs").join(blob_name)) {
                Ok(_) => true,
                Err(cause) if cause.kind() == std::io::ErrorKind::NotFound => false,
                Err(cause) => {
                    return Err(error(
                        "recovery_unavailable",
                        format!("cannot inspect recovery blob admission target: {cause}"),
                    ));
                }
            }
        } else {
            false
        };
        if !already_exists {
            let length = u64::try_from(text.len()).map_err(|_| {
                error(
                    "recovery_capacity_exceeded",
                    "recovery body length exceeds the storage budget representation",
                )
            })?;
            additional_bytes = additional_bytes.saturating_add(length);
        }
    }
    if usage.body_bytes.saturating_add(additional_bytes) > limits.max_body_bytes {
        return Err(error(
            "recovery_capacity_exceeded",
            "durable editor recovery body-byte limit is reached",
        ));
    }
    Ok(())
}

fn write_recovery_checkpoint(
    recovery_root: &Path,
    mut manifest: FileRecoveryManifestV1,
    base: &str,
    buffer: &str,
    now: u64,
    fail_before_manifest: bool,
    orphan_grace_period: Duration,
) -> Result<FileRecoveryManifestV1, FileResourceErrorV1> {
    let record_dir = recovery_record_dir(recovery_root, &manifest.recovery_id)?;
    std::fs::create_dir_all(recovery_root).map_err(|cause| {
        error(
            "recovery_unavailable",
            format!("cannot create recovery root: {cause}"),
        )
    })?;
    validate_recovery_root(recovery_root)?;
    std::fs::create_dir(&record_dir)
        .or_else(|cause| {
            if cause.kind() == std::io::ErrorKind::AlreadyExists {
                Ok(())
            } else {
                Err(cause)
            }
        })
        .map_err(|cause| {
            error(
                "recovery_unavailable",
                format!("cannot create recovery directory: {cause}"),
            )
        })?;
    validate_recovery_record_dir(recovery_root, &manifest.recovery_id)?;
    manifest.base_blob = write_recovery_blob(&record_dir, base)?;
    manifest.buffer_blob = write_recovery_blob(&record_dir, buffer)?;
    manifest.recovery_revision = manifest
        .recovery_revision
        .checked_add(1)
        .ok_or_else(|| error("recovery_conflict", "recovery revision is exhausted"))?;
    manifest.updated_at_ms = now;
    if fail_before_manifest {
        return Err(error(
            "recovery_unavailable",
            "injected failure before recovery manifest replacement",
        ));
    }
    wardian_core::conversations::write_json_atomic(&record_dir.join("manifest.json"), &manifest)
        .map_err(|cause| {
            error(
                "recovery_unavailable",
                format!("cannot commit recovery manifest: {cause}"),
            )
        })?;
    garbage_collect_recovery_blobs(recovery_root, &manifest, orphan_grace_period);
    Ok(manifest)
}

#[cfg(not(windows))]
fn sync_recovery_directory(path: &Path) -> std::io::Result<()> {
    File::open(path)?.sync_all()
}

#[cfg(windows)]
fn sync_recovery_directory(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

fn garbage_collect_recovery_blobs(
    recovery_root: &Path,
    manifest: &FileRecoveryManifestV1,
    orphan_grace_period: Duration,
) {
    let Ok(record_dir) = validate_recovery_record_dir(recovery_root, &manifest.recovery_id) else {
        return;
    };
    let blobs_dir = record_dir.join("blobs");
    if validate_direct_child_directory(&record_dir, &blobs_dir, "blob directory").is_err() {
        return;
    }
    let Ok(entries) = std::fs::read_dir(blobs_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !is_recovery_blob_name(name)
            || name == manifest.base_blob
            || name == manifest.buffer_blob
        {
            continue;
        }
        let Ok(metadata) = entry.path().symlink_metadata() else {
            continue;
        };
        if !metadata.file_type().is_file()
            || metadata
                .modified()
                .ok()
                .and_then(|modified| modified.elapsed().ok())
                .is_none_or(|age| age < orphan_grace_period)
        {
            continue;
        }
        let _ = std::fs::remove_file(entry.path());
    }
}

fn remove_recovery_record(
    recovery_root: &Path,
    recovery_id: &str,
) -> Result<(), FileResourceErrorV1> {
    let record_dir = validate_recovery_record_dir(recovery_root, recovery_id)?;
    let root = std::fs::canonicalize(recovery_root).map_err(|cause| {
        error(
            "recovery_unavailable",
            format!("cannot resolve recovery root before discard: {cause}"),
        )
    })?;
    let record = std::fs::canonicalize(&record_dir).map_err(|cause| {
        if cause.kind() == std::io::ErrorKind::NotFound {
            error("recovery_not_found", "recovery checkpoint does not exist")
        } else {
            error(
                "recovery_unavailable",
                format!("cannot resolve recovery checkpoint before discard: {cause}"),
            )
        }
    })?;
    let metadata = std::fs::symlink_metadata(&record).map_err(|cause| {
        error(
            "recovery_unavailable",
            format!("cannot inspect recovery checkpoint before discard: {cause}"),
        )
    })?;
    if !metadata.file_type().is_dir() || record.parent() != Some(root.as_path()) {
        return Err(error(
            "invalid_recovery",
            "recovery checkpoint is outside the configured recovery root",
        ));
    }
    std::fs::remove_dir_all(&record).map_err(|cause| {
        error(
            "recovery_unavailable",
            format!("cannot discard recovery checkpoint: {cause}"),
        )
    })
}
