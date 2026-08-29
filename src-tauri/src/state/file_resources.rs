//! Backend-owned file subscriptions, stable revisions, and bounded read leases.

use notify::Watcher as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, RwLock, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
#[cfg(windows)]
use std::{ffi::OsStr, os::windows::ffi::OsStrExt as _};
use tauri::{Emitter as _, Manager as _};
use tokio::sync::{broadcast, Mutex, OwnedMutexGuard};
use uuid::Uuid;
use wardian_core::files::{
    AuthorizedPath, AuthorizedRootService, FileContentDescriptorV1, FileRendererKind,
    FileResourceErrorV1, FileResourceLimits, FileRevisionToken, VerifiedFileSnapshot,
};
use wardian_core::models::AgentConfig;

pub const FILE_RESOURCE_REVISION_EVENT: &str = "file-resource://revision";
const DEFAULT_STABILITY_DELAY: Duration = Duration::from_millis(150);
const DEFAULT_TICKET_TTL: Duration = Duration::from_secs(60);
const DEFAULT_MAX_USER_FILE_GRANTS: usize = 128;
const USER_FILE_GRANT_STORE_SCHEMA_VERSION: u32 = 1;
const DEFAULT_MAX_SAVE_TARGET_GRANTS: usize = 32;
const DEFAULT_SAVE_TARGET_TTL: Duration = Duration::from_secs(60);
const MAX_TICKET_SNAPSHOT_BYTES: u64 = 1024 * 1024 * 1024;
const MIN_TICKET_SNAPSHOT_RESERVATION_BYTES: u64 = 4 * 1024 * 1024;
const RECOVERY_ORPHAN_GRACE_PERIOD: Duration = Duration::from_secs(24 * 60 * 60);
const DEFAULT_MAX_RECOVERY_RECORDS: usize = 128;
const DEFAULT_MAX_RECOVERY_BODY_BYTES: u64 = 512 * 1024 * 1024;

include!("file_resources/wire.rs");
include!("file_resources/recovery.rs");
include!("file_resources/grants.rs");
include!("file_resources/tickets.rs");
include!("file_resources/runtime.rs");

#[cfg(test)]
mod tests {
    include!("file_resources/tests.rs");
}
