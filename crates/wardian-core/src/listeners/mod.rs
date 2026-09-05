//! Persistence, validation, and matching for automation **listener** invokers.
//!
//! A listener is a persisted invoker that watches something and fires a
//! durable automation run when it changes. It supplies the same invocation
//! contract every other invoker does — `{ blueprint, input, bindings,
//! provider, workspace, assignments }` — so a listener run is an ordinary run.
//!
//! Three trigger variants share this one family: [`FileWatchTrigger`],
//! [`WebhookTrigger`], and [`WebPollTrigger`]. A fourth costs an enum variant
//! rather than a parallel subsystem.
//!
//! This module is pure: no Tauri, no network, no filesystem watching. The
//! effect layer lives in `src-tauri/src/automation/listener/`.

use crate::models::AutomationAssignments;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs::OpenOptions;

pub mod file;
pub mod poll;
pub mod secrets;
pub mod webhook;

/// Debounce window collapsing one burst of filesystem events into one fire.
pub const DEFAULT_DEBOUNCE_MS: u32 = 500;

/// Upper bound on the debounce window. Beyond this a listener stops being a
/// listener and should be a schedule.
pub const MAX_DEBOUNCE_MS: u32 = 60_000;

/// Politeness floor for outbound polling. Nothing a user watches over HTTP
/// changes usefully faster than this, and a tighter loop is indistinguishable
/// from abuse to the operator on the other end.
pub const MIN_POLL_INTERVAL_SECONDS: u32 = 30;

/// A year of seconds; anything longer is a schedule, not a poll.
pub const MAX_POLL_INTERVAL_SECONDS: u32 = 31_536_000;

pub const DEFAULT_WEBHOOK_MAX_BODY_BYTES: u32 = 256 * 1024;
pub const MAX_WEBHOOK_MAX_BODY_BYTES: u32 = 8 * 1024 * 1024;
pub const DEFAULT_POLL_MAX_BODY_BYTES: u32 = 1024 * 1024;
pub const MAX_POLL_MAX_BODY_BYTES: u32 = 8 * 1024 * 1024;

/// Fires allowed inside [`RATE_CEILING_WINDOW_MS`] before a listener is
/// auto-disabled. This is the backstop for a self-triggering file watcher and
/// for a webhook flood under `parallel` overlap; static path checks cannot
/// catch every loop, and a runaway listener spends real provider tokens.
pub const RATE_CEILING_FIRES: u32 = 20;
pub const RATE_CEILING_WINDOW_MS: u64 = 60_000;

/// What a listener does when it fires while one of its own runs is still going.
///
/// The vocabulary deliberately matches the policies proposed in the general
/// run-concurrency contract (#1008) so that contract can absorb these rather
/// than compete with them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlapPolicy {
    /// Drop the new fire while a run is active.
    Skip,
    /// Keep at most one pending fire, replacing any earlier pending one, so a
    /// burst can never grow an unbounded queue.
    Coalesce,
    /// Start the run regardless.
    Parallel,
}

/// Which filesystem changes a file listener reacts to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileChangeKind {
    Created,
    Modified,
    Removed,
}

/// How a webhook sender proves it is allowed to fire the listener.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebhookAuth {
    /// Shared bearer token, compared in constant time.
    Token,
    /// `HMAC-SHA256` over the raw body, as GitHub and Stripe send it.
    HmacSha256,
}

/// HTTP method a poll listener issues.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PollMethod {
    Get,
    Head,
}

/// What counts as "the watched resource changed".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FingerprintSource {
    /// `ETag`, falling back to `Last-Modified`, falling back to a body hash.
    /// Cheapest, and correct for most servers.
    EtagOrLastModified,
    /// SHA-256 of the (capped) response body.
    BodyHash,
    /// RFC 6901 JSON pointer into a JSON body. The "notify me when they cut a
    /// release" case: `/0/tag_name` against a releases array.
    JsonPointer,
    /// First capture group of a regular expression against a text body.
    Regex,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileWatchTrigger {
    /// Absolute path to the watched file or directory.
    pub path: String,
    #[serde(default)]
    pub recursive: bool,
    /// Globs, relative to `path`, that a change must match. Empty means all.
    #[serde(default)]
    pub patterns: Vec<String>,
    /// Globs excluded from matching, merged *over* the built-in defaults.
    #[serde(default)]
    pub ignore: Vec<String>,
    /// Change kinds that fire. Empty means all.
    #[serde(default)]
    pub events: Vec<FileChangeKind>,
    #[serde(default = "default_debounce_ms")]
    pub debounce_ms: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebhookTrigger {
    /// The listener is reachable at `/hooks/<path_segment>`.
    pub path_segment: String,
    pub auth: WebhookAuth,
    /// Header carrying the signature for [`WebhookAuth::HmacSha256`].
    /// Defaults to `X-Hub-Signature-256`.
    #[serde(default)]
    pub signature_header: Option<String>,
    #[serde(default = "default_webhook_max_body_bytes")]
    pub max_body_bytes: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WebPollTrigger {
    pub url: String,
    #[serde(default = "default_poll_interval_seconds")]
    pub interval_seconds: u32,
    #[serde(default = "default_poll_method")]
    pub method: PollMethod,
    /// Non-secret request headers. Credential-bearing headers belong in
    /// [`secrets`], never in this inspectable file.
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default = "default_fingerprint_source")]
    pub fingerprint: FingerprintSource,
    #[serde(default)]
    pub json_pointer: Option<String>,
    #[serde(default)]
    pub regex: Option<String>,
    #[serde(default = "default_poll_max_body_bytes")]
    pub max_body_bytes: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ListenerTrigger {
    FileWatch(FileWatchTrigger),
    Webhook(WebhookTrigger),
    WebPoll(WebPollTrigger),
}

impl ListenerTrigger {
    /// Stable discriminant for logs, CLI output, and UI grouping.
    pub fn kind(&self) -> &'static str {
        match self {
            ListenerTrigger::FileWatch(_) => "file_watch",
            ListenerTrigger::Webhook(_) => "webhook",
            ListenerTrigger::WebPoll(_) => "web_poll",
        }
    }

    /// Overlap default appropriate to how this variant's events relate.
    ///
    /// File events in one burst describe a single logical change, so skipping
    /// a concurrent fire loses nothing. Webhook deliveries are independent
    /// events with distinct payloads, so skipping would silently drop real
    /// deliveries; retries do not fan out because idempotency collapses them
    /// onto the same run, and the rate ceiling bounds the rest.
    pub fn default_overlap(&self) -> OverlapPolicy {
        match self {
            ListenerTrigger::Webhook(_) => OverlapPolicy::Parallel,
            _ => OverlapPolicy::Skip,
        }
    }
}

/// Why a listener refused an event, kept so that "my webhook isn't firing" is
/// a debuggable question rather than a guess.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListenerRejection {
    pub reason: String,
    pub at_epoch_ms: u64,
}

/// Every app-written field, in one place.
///
/// Keeping runtime state out of the trigger config means no field has two
/// writers: the user owns configuration, the app owns this, and the write-back
/// merge is a single field copy.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListenerRuntime {
    /// Whether the effect layer currently has this listener watching.
    #[serde(default)]
    pub armed: bool,
    #[serde(default)]
    pub arm_error: Option<String>,
    #[serde(default)]
    pub last_fire_epoch_ms: Option<u64>,
    #[serde(default)]
    pub last_run_status: Option<String>,
    #[serde(default)]
    pub last_run_error: Option<String>,
    #[serde(default)]
    pub last_rejection: Option<ListenerRejection>,
    #[serde(default)]
    pub fire_count: u64,
    /// Epoch-ms of recent fires, trimmed to the rate-ceiling window.
    #[serde(default)]
    pub recent_fire_epoch_ms: Vec<u64>,
    /// Set when the rate ceiling trips. `enabled` is never written by the app,
    /// so a user re-enabling concurrently cannot race an auto-disable.
    #[serde(default)]
    pub disabled_reason: Option<String>,
    // ---- web poll only ----
    #[serde(default)]
    pub poll_fingerprint: Option<String>,
    #[serde(default)]
    pub next_poll_epoch_ms: Option<u64>,
    #[serde(default)]
    pub consecutive_failures: u32,
}

/// A persisted listener invoker.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutomationListener {
    pub id: String,
    /// Resolves through the same recursive-by-id blueprint resolver the manual
    /// and scheduled run paths use.
    pub blueprint_id: String,
    pub name: String,
    /// User-owned. The app never writes this field.
    #[serde(default)]
    pub enabled: bool,
    pub trigger: ListenerTrigger,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub workspace: Option<String>,
    /// Entry input merged under the event payload the listener contributes.
    #[serde(default)]
    pub input: serde_json::Value,
    #[serde(default)]
    pub bindings: HashMap<String, String>,
    #[serde(default)]
    pub assignments: AutomationAssignments,
    #[serde(default)]
    pub overlap: Option<OverlapPolicy>,
    #[serde(default)]
    pub runtime: ListenerRuntime,
}

impl AutomationListener {
    /// The overlap policy in force, resolving an unset value to the variant's
    /// default rather than to a single global one.
    pub fn effective_overlap(&self) -> OverlapPolicy {
        self.overlap
            .unwrap_or_else(|| self.trigger.default_overlap())
    }

    /// Whether the effect layer should arm this listener.
    ///
    /// `enabled` is the user's switch and `disabled_reason` is the app's, so
    /// both must agree. Re-enabling from the CLI or UI clears the latter.
    pub fn should_arm(&self) -> bool {
        self.enabled && self.runtime.disabled_reason.is_none()
    }
}

fn default_debounce_ms() -> u32 {
    DEFAULT_DEBOUNCE_MS
}
fn default_webhook_max_body_bytes() -> u32 {
    DEFAULT_WEBHOOK_MAX_BODY_BYTES
}
fn default_poll_max_body_bytes() -> u32 {
    DEFAULT_POLL_MAX_BODY_BYTES
}
fn default_poll_interval_seconds() -> u32 {
    300
}
fn default_poll_method() -> PollMethod {
    PollMethod::Get
}
fn default_fingerprint_source() -> FingerprintSource {
    FingerprintSource::EtagOrLastModified
}

#[derive(Serialize, Deserialize)]
struct ListenerFile {
    #[serde(default = "default_schema")]
    schema: u8,
    #[serde(default)]
    listeners: Vec<AutomationListener>,
}

fn default_schema() -> u8 {
    1
}

/// Read all listeners, preserving storage and parse failures for callers that
/// cannot safely treat unavailable authoritative state as empty.
pub fn try_load_listeners() -> Result<Vec<AutomationListener>, String> {
    let Some(path) = crate::paths::listeners_path() else {
        return Err("no wardian home".to_string());
    };
    if !path.exists() {
        return Ok(Vec::new());
    }
    let content = std::fs::read_to_string(&path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    serde_json::from_str::<ListenerFile>(&content)
        .map(|file| file.listeners)
        .map_err(|error| format!("failed to parse {}: {error}", path.display()))
}

/// Read all listeners. A missing or malformed file yields an empty list and a
/// stderr note, matching `schedule::load_schedules`.
pub fn load_listeners() -> Vec<AutomationListener> {
    match try_load_listeners() {
        Ok(listeners) => listeners,
        Err(error) => {
            eprintln!("[wardian-core] {error}");
            Vec::new()
        }
    }
}

/// Serialize a whole read-modify-write across the app and CLI processes.
///
/// Atomic replacement alone prevents torn JSON, not lost updates: without the
/// lock, a CLI edit and an app runtime write-back read the same bytes and the
/// later writer silently discards the other's change.
pub fn mutate_listeners<T>(
    mutate: impl FnOnce(&mut Vec<AutomationListener>) -> std::io::Result<T>,
) -> std::io::Result<T> {
    let path = crate::paths::listeners_path().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "Wardian home is unavailable")
    })?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let lock = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path.with_extension("lock"))?;
    FileExt::lock_exclusive(&lock)?;
    let mut listeners = read_for_mutation(&path)?;
    let result = mutate(&mut listeners)?;
    write_listeners(&path, &listeners)?;
    Ok(result)
}

/// Read the current listener set for a read-modify-write, failing closed on a
/// file that exists but cannot be parsed.
///
/// Treating a malformed file as an empty set would be catastrophic here rather
/// than merely lossy: the caller writes the result back, so a single runtime
/// update - a fire, an arming result, a rejection - would replace every
/// configured listener with nothing. A missing file is still an empty set,
/// because that is a fresh install rather than damage.
fn read_for_mutation(path: &std::path::Path) -> std::io::Result<Vec<AutomationListener>> {
    match std::fs::read_to_string(path) {
        Ok(body) => serde_json::from_str::<ListenerFile>(&body)
            .map(|file| file.listeners)
            .map_err(|error| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "refusing to overwrite malformed {}: {error}",
                        path.display()
                    ),
                )
            }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(error),
    }
}

fn write_listeners(
    path: &std::path::Path,
    listeners: &[AutomationListener],
) -> std::io::Result<()> {
    let file = ListenerFile {
        schema: default_schema(),
        listeners: listeners.to_vec(),
    };
    let body = serde_json::to_string_pretty(&file)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Replace the whole listener set.
pub fn save_listeners(listeners: &[AutomationListener]) -> std::io::Result<()> {
    mutate_listeners(|stored| {
        *stored = listeners.to_vec();
        Ok(())
    })
}

/// Fail-closed validation applied identically to CLI and UI writes.
///
/// Both surfaces construct this DTO, so the check validates the serialized
/// record rather than trusting a particular caller's controls.
pub fn validate_listener(listener: &AutomationListener) -> Result<(), String> {
    if listener.name.trim().is_empty() {
        return Err("listener name must not be empty".to_string());
    }
    if listener.blueprint_id.trim().is_empty() {
        return Err("listener blueprint must not be empty".to_string());
    }
    match &listener.trigger {
        ListenerTrigger::FileWatch(trigger) => file::validate(trigger),
        ListenerTrigger::Webhook(trigger) => webhook::validate(trigger),
        ListenerTrigger::WebPoll(trigger) => poll::validate(trigger),
    }
}

/// Whether this fire pushes the listener past the rate ceiling.
///
/// Returns the trimmed fire window so the caller can persist it. A listener at
/// the ceiling is auto-disabled rather than throttled: silent throttling hides
/// a loop, and the loop is the thing worth surfacing.
pub fn record_fire_within_ceiling(runtime: &mut ListenerRuntime, now_ms: u64) -> bool {
    let cutoff = now_ms.saturating_sub(RATE_CEILING_WINDOW_MS);
    runtime.recent_fire_epoch_ms.retain(|at| *at > cutoff);
    runtime.recent_fire_epoch_ms.push(now_ms);
    runtime.fire_count = runtime.fire_count.saturating_add(1);
    runtime.last_fire_epoch_ms = Some(now_ms);
    if runtime.recent_fire_epoch_ms.len() as u32 > RATE_CEILING_FIRES {
        runtime.disabled_reason = Some(format!(
            "auto-disabled after {} fires in {} seconds; check for a self-triggering watch path or an event flood",
            runtime.recent_fire_epoch_ms.len(),
            RATE_CEILING_WINDOW_MS / 1000
        ));
        return false;
    }
    true
}

/// Fingerprint of every field that affects *arming*, so the supervisor can
/// skip re-arming when only runtime state changed.
///
/// Without this, each fire's runtime write-back would look like a config
/// change and tear down every watcher: a listener would disarm itself by
/// working.
pub fn arming_fingerprint(listeners: &[AutomationListener]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    let mut ids: Vec<&AutomationListener> = listeners.iter().collect();
    ids.sort_by(|left, right| left.id.cmp(&right.id));
    for listener in ids {
        hasher.update(listener.id.as_bytes());
        hasher.update([u8::from(listener.should_arm())]);
        // Serialization of the trigger is the config surface that arming
        // depends on; runtime deliberately is not hashed.
        let trigger = serde_json::to_string(&listener.trigger).unwrap_or_default();
        hasher.update(trigger.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
pub(crate) mod test_support {
    use super::*;

    pub fn listener(id: &str, trigger: ListenerTrigger) -> AutomationListener {
        AutomationListener {
            id: id.into(),
            blueprint_id: "audit".into(),
            name: "Audit".into(),
            enabled: true,
            trigger,
            provider: None,
            workspace: None,
            input: serde_json::json!({}),
            bindings: HashMap::new(),
            assignments: AutomationAssignments::new(),
            overlap: None,
            runtime: ListenerRuntime::default(),
        }
    }

    pub fn file_trigger(path: &str) -> ListenerTrigger {
        ListenerTrigger::FileWatch(FileWatchTrigger {
            path: path.into(),
            recursive: true,
            patterns: Vec::new(),
            ignore: Vec::new(),
            events: Vec::new(),
            debounce_ms: DEFAULT_DEBOUNCE_MS,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{file_trigger, listener};
    use super::*;

    struct TestHome {
        _guard: std::sync::MutexGuard<'static, ()>,
        _home: tempfile::TempDir,
        previous: Option<std::ffi::OsString>,
    }

    impl TestHome {
        fn new() -> Self {
            let guard = crate::tests::env_lock();
            let home = tempfile::tempdir().expect("temp wardian home");
            let previous = std::env::var_os("WARDIAN_HOME");
            std::env::set_var("WARDIAN_HOME", home.path());
            Self {
                _guard: guard,
                _home: home,
                previous,
            }
        }
    }

    impl Drop for TestHome {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(value) => std::env::set_var("WARDIAN_HOME", value),
                None => std::env::remove_var("WARDIAN_HOME"),
            }
        }
    }

    #[test]
    fn save_then_load_round_trips_every_variant() {
        let _home = TestHome::new();
        let listeners = vec![
            listener("file", file_trigger("/tmp/watched")),
            listener(
                "hook",
                ListenerTrigger::Webhook(WebhookTrigger {
                    path_segment: "ci".into(),
                    auth: WebhookAuth::HmacSha256,
                    signature_header: None,
                    max_body_bytes: DEFAULT_WEBHOOK_MAX_BODY_BYTES,
                }),
            ),
            listener(
                "poll",
                ListenerTrigger::WebPoll(WebPollTrigger {
                    url: "https://example.invalid/releases".into(),
                    interval_seconds: 300,
                    method: PollMethod::Get,
                    headers: BTreeMap::new(),
                    fingerprint: FingerprintSource::JsonPointer,
                    json_pointer: Some("/0/tag_name".into()),
                    regex: None,
                    max_body_bytes: DEFAULT_POLL_MAX_BODY_BYTES,
                }),
            ),
        ];
        save_listeners(&listeners).expect("save");
        let loaded = load_listeners();
        assert_eq!(loaded.len(), 3);
        assert_eq!(loaded, listeners);
    }

    #[test]
    fn missing_file_loads_empty_but_malformed_file_is_preserved_as_an_error() {
        let _home = TestHome::new();
        assert!(load_listeners().is_empty());
        let path = crate::paths::listeners_path().expect("path");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("library dir");
        std::fs::write(&path, "not json").expect("write");
        assert!(try_load_listeners()
            .unwrap_err()
            .contains("failed to parse"));
        assert!(load_listeners().is_empty());
    }

    #[test]
    fn a_runtime_write_refuses_to_overwrite_a_malformed_config() {
        let _home = TestHome::new();
        let path = crate::paths::listeners_path().expect("path");
        std::fs::create_dir_all(path.parent().expect("parent")).expect("library dir");
        std::fs::write(&path, "{ not json").expect("seed damage");

        // A fire, an arming result, or a rejection all reach this mutator. If
        // it treated a malformed file as an empty set it would write that back
        // and destroy every configured listener.
        let error = mutate_listeners(|stored| {
            stored.clear();
            Ok(())
        })
        .expect_err("a malformed config must fail closed");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("refusing to overwrite"));
        assert_eq!(
            std::fs::read_to_string(&path).expect("read back"),
            "{ not json",
            "the damaged file must be left exactly as it was"
        );
    }

    #[test]
    fn a_missing_config_is_still_an_empty_set_rather_than_an_error() {
        let _home = TestHome::new();
        mutate_listeners(|stored| {
            stored.push(listener("a", file_trigger("/tmp/watched")));
            Ok(())
        })
        .expect("a fresh install writes its first listener");
        assert_eq!(load_listeners().len(), 1);
    }

    #[test]
    fn runtime_write_back_does_not_change_the_arming_fingerprint() {
        let mut listeners = vec![listener("file", file_trigger("/tmp/watched"))];
        let before = arming_fingerprint(&listeners);
        listeners[0].runtime.fire_count = 42;
        listeners[0].runtime.last_fire_epoch_ms = Some(1_000);
        listeners[0].runtime.last_run_status = Some("completed".into());
        assert_eq!(before, arming_fingerprint(&listeners));

        listeners[0].enabled = false;
        assert_ne!(before, arming_fingerprint(&listeners));
    }

    #[test]
    fn auto_disable_never_writes_the_user_owned_enabled_flag() {
        let mut subject = listener("file", file_trigger("/tmp/watched"));
        for tick in 0..=u64::from(RATE_CEILING_FIRES) {
            record_fire_within_ceiling(&mut subject.runtime, 1_000 + tick);
        }
        assert!(subject.runtime.disabled_reason.is_some());
        assert!(subject.enabled, "the app must not write `enabled`");
        assert!(!subject.should_arm());
    }

    #[test]
    fn fires_outside_the_window_do_not_accumulate_toward_the_ceiling() {
        let mut runtime = ListenerRuntime::default();
        for tick in 0..u64::from(RATE_CEILING_FIRES) {
            assert!(record_fire_within_ceiling(
                &mut runtime,
                tick * RATE_CEILING_WINDOW_MS * 2
            ));
        }
        assert!(runtime.disabled_reason.is_none());
        assert_eq!(runtime.fire_count, u64::from(RATE_CEILING_FIRES));
    }

    #[test]
    fn overlap_defaults_follow_how_each_variant_relates_its_events() {
        let file = listener("file", file_trigger("/tmp/watched"));
        assert_eq!(file.effective_overlap(), OverlapPolicy::Skip);

        let hook = listener(
            "hook",
            ListenerTrigger::Webhook(WebhookTrigger {
                path_segment: "ci".into(),
                auth: WebhookAuth::Token,
                signature_header: None,
                max_body_bytes: DEFAULT_WEBHOOK_MAX_BODY_BYTES,
            }),
        );
        assert_eq!(hook.effective_overlap(), OverlapPolicy::Parallel);

        let explicit = AutomationListener {
            overlap: Some(OverlapPolicy::Coalesce),
            ..hook
        };
        assert_eq!(explicit.effective_overlap(), OverlapPolicy::Coalesce);
    }

    #[test]
    fn a_config_replacement_preserves_runtime_written_after_the_caller_read_it() {
        // Models what `listener_save` does. The editor holds a snapshot from
        // before the user started typing; a fire can land in between. Taking
        // runtime from the locked current record rather than from that snapshot
        // is what keeps the poll fingerprint, rate-ceiling window, and arming
        // state from being reset by an unrelated rename.
        let _home = TestHome::new();
        save_listeners(&[listener("a", file_trigger("/tmp/watched"))]).expect("seed");
        let stale_snapshot = load_listeners().remove(0);

        mutate_listeners(|stored| {
            stored[0].runtime.fire_count = 7;
            stored[0].runtime.poll_fingerprint = Some("etag:current".into());
            Ok(())
        })
        .expect("a fire lands while the editor is open");

        mutate_listeners(|stored| {
            let mut record = stale_snapshot.clone();
            record.name = "renamed".into();
            if let Some(existing) = stored.iter_mut().find(|item| item.id == record.id) {
                record.runtime = existing.runtime.clone();
                *existing = record;
            }
            Ok(())
        })
        .expect("save");

        let saved = load_listeners().remove(0);
        assert_eq!(saved.name, "renamed", "the config edit applies");
        assert_eq!(saved.runtime.fire_count, 7, "the fire is not rolled back");
        assert_eq!(
            saved.runtime.poll_fingerprint.as_deref(),
            Some("etag:current")
        );
    }

    #[test]
    fn concurrent_config_and_runtime_writes_both_survive() {
        let _home = TestHome::new();
        save_listeners(&[listener("a", file_trigger("/tmp/watched"))]).expect("seed");

        mutate_listeners(|stored| {
            stored[0].name = "renamed".into();
            Ok(())
        })
        .expect("config edit");
        mutate_listeners(|stored| {
            stored[0].runtime.fire_count = 7;
            Ok(())
        })
        .expect("runtime write-back");

        let loaded = load_listeners();
        assert_eq!(loaded[0].name, "renamed");
        assert_eq!(loaded[0].runtime.fire_count, 7);
    }
}
