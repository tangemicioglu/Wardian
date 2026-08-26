use crate::providers::antigravity::{changed_workspace_conversation, AntigravityProvider};
use crate::providers::claude::{classify_claude_user_event, ClaudeUserEventKind};
use crate::providers::codex::CodexProvider;
use crate::providers::pi::PiProvider;
use crate::providers::transcript::extract_transcript_message;
use crate::providers::ProviderFactory;
use crate::state::{ActiveAgent, AgentWatchState, AppState};
use crate::utils::fs::*;
use crate::utils::logging::{log_debug, log_terminal_trace_bytes, log_terminal_trace_note};
use crate::utils::PtyUtf8Decoder;
#[cfg(test)]
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
#[cfg(test)]
use std::io::Write;
use std::io::{BufRead, Read, Seek};
use tauri::{AppHandle, Emitter, Manager};
use wardian_core::control::ProviderInputReadiness;
use wardian_core::models::{AgentConfig, AgentEvent, ProviderConfig};

use super::claude::{
    claude_log_paths, claude_permission_hook_matches_session, claude_project_dir_name,
    discover_claude_log_for_session_name,
};
use super::codex::{codex_provider_session_is_excluded, codex_session_file_path};
use super::opencode::{
    opencode_interactive_env, opencode_recent_session_for_workspace, opencode_status_from_title,
};
use super::session_identity::{
    apply_provider_identity, expected_caller_owned_identity, ProviderIdentityOutcome,
};
use super::{
    apply_agent_event, apply_agent_event_with_policy, apply_agent_status_event,
    apply_agent_status_event_with_policy, debug_preview_bytes, extract_terminal_titles,
    finalize_interactive_spawn_args, interactive_provider_args, interactive_provider_cwd,
    interactive_provider_launch, set_agent_status, ProviderStatusEventPolicy,
};
use crate::providers::gemini::gemini_status_from_title;

const OUTPUT_READY_EMIT_MIN_INTERVAL: std::time::Duration = std::time::Duration::from_millis(33);

type PendingMemoryInjection = (
    wardian_core::memory::MemoryStore,
    wardian_core::memory::CompiledMemoryBrief,
    String,
    String,
);

fn record_pending_memory_injection(
    pending: &mut Option<PendingMemoryInjection>,
    agent_id: &str,
    provider: &str,
) -> bool {
    let Some((store, brief, workspace, process_key)) = pending.take() else {
        return false;
    };
    if let Err(error) = store.record_injection(
        &wardian_core::memory::MemoryActor::agent(agent_id),
        agent_id,
        Some(&workspace),
        provider,
        &process_key,
        &brief,
    ) {
        log_debug(&format!(
            "[Wardian] memory injection receipt unavailable for {agent_id}: {error}"
        ));
    }
    true
}

fn provider_title_has_startup_ready_prompt(provider: &str, title: &str, status: &str) -> bool {
    if provider != "opencode" || wardian_core::identity::normalize_status(status) != "idle" {
        return false;
    }
    let title = title.trim();
    title == "OpenCode" || title.starts_with("OC | ")
}

#[derive(Default)]
struct OpenCodeStartupMemoryTransition {
    ready_observed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProviderTerminalObservationSource {
    RawProviderStream,
    RenderedZellijFrame,
}

impl ProviderTerminalObservationSource {
    fn carries_provider_events(self) -> bool {
        self == Self::RawProviderStream
    }
}

fn observe_antigravity_terminal_completion(
    source: ProviderTerminalObservationSource,
    gate: &mut AntigravityTurnCompletionGate,
    provider_name: &str,
    current_status: &str,
    output: &str,
) -> bool {
    source.carries_provider_events() && gate.observe_output(provider_name, current_status, output)
}

impl OpenCodeStartupMemoryTransition {
    /// Classify the real provider title and promote the pending memory receipt
    /// exactly once when that title proves the compose surface is ready.
    fn observe_title(
        &mut self,
        pending: &mut Option<PendingMemoryInjection>,
        provider: &str,
        title: &str,
        agent_id: &str,
    ) -> Option<&'static str> {
        let status = opencode_status_from_title(title)?;
        if !self.ready_observed && provider_title_has_startup_ready_prompt(provider, title, status)
        {
            self.ready_observed = true;
            record_pending_memory_injection(pending, agent_id, provider);
        }
        Some(status)
    }

    /// Zellij owns the raw PTY stream, so its rendered-frame consumer cannot
    /// observe OSC titles. The provider-owned OpenCode log remains the status
    /// authority; promote startup only after that channel reports idle.
    fn observe_provider_status(
        &mut self,
        pending: &mut Option<PendingMemoryInjection>,
        provider: &str,
        status: &str,
        agent_id: &str,
    ) -> bool {
        if self.ready_observed
            || provider != "opencode"
            || wardian_core::identity::normalize_status(status) != "idle"
        {
            return false;
        }
        self.ready_observed = true;
        record_pending_memory_injection(pending, agent_id, provider);
        true
    }
}

/// Selects the verified Antigravity conversation created by this launch for
/// log discovery and whether it should be persisted as the resume identity.
/// A workspace mapping that existed before launch belongs to the prior
/// provider conversation, so it must not be replayed by a fresh launch.
/// Where the mock provider mirrors its event stream.
///
/// Real providers are observed through a log they own, and the chat transcript
/// reads normalized events back from that log alone. The mock provider writes
/// only to the PTY, so without a log of its own its tool calls could never
/// reach the transcript and the chat surface stayed untestable offline.
fn mock_transcript_log_path(session_id: &str) -> Option<std::path::PathBuf> {
    // Reuses the conversations directory's own safety check on the id rather
    // than joining an unvalidated path component under the Wardian home.
    wardian_core::paths::agent_conversations_dir(session_id)
        .and_then(|dir| dir.parent().map(|agent_dir| agent_dir.to_path_buf()))
        .map(|dir| dir.join("mock-transcript.jsonl"))
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PiLogBaseline {
    path: std::path::PathBuf,
    cursor: PiLogCursor,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct PiLogCursor {
    offset: u64,
    identity: Option<PiFileIdentity>,
    boundary_start: u64,
    boundary: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PiFileIdentity(u64, u64);

fn pi_file_identity(metadata: &std::fs::Metadata) -> Option<PiFileIdentity> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Some(PiFileIdentity(metadata.dev(), metadata.ino()))
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // Stable Rust does not expose Windows' by-handle file index yet.
        // Creation time is stable across appends, while the boundary check below
        // also detects in-place rewrites and same-timestamp replacements.
        Some(PiFileIdentity(metadata.creation_time(), 0))
    }
}

fn pi_log_boundary(file: &mut std::fs::File, offset: u64) -> Option<(u64, Vec<u8>)> {
    const BOUNDARY_BYTES: u64 = 4096;
    let boundary_start = offset.saturating_sub(BOUNDARY_BYTES);
    let boundary_len = offset.saturating_sub(boundary_start);
    file.seek(std::io::SeekFrom::Start(boundary_start)).ok()?;
    let mut boundary = Vec::new();
    std::io::Read::by_ref(file)
        .take(boundary_len)
        .read_to_end(&mut boundary)
        .ok()?;
    (boundary.len() as u64 == boundary_len).then_some((boundary_start, boundary))
}

fn refresh_pi_log_boundary(file: &mut std::fs::File, cursor: &mut PiLogCursor) -> Option<()> {
    let (boundary_start, boundary) = pi_log_boundary(file, cursor.offset)?;
    cursor.boundary_start = boundary_start;
    cursor.boundary = boundary;
    Some(())
}

fn pi_log_baseline(
    session_dir: &std::path::Path,
    provider_session_id: &str,
) -> Option<PiLogBaseline> {
    let path = PiProvider::session_file(session_dir, provider_session_id)?;
    let mut file = std::fs::File::open(&path).ok()?;
    let metadata = file.metadata().ok()?;
    let mut cursor = PiLogCursor {
        offset: metadata.len(),
        identity: pi_file_identity(&metadata),
        ..Default::default()
    };
    refresh_pi_log_boundary(&mut file, &mut cursor)?;
    Some(PiLogBaseline { path, cursor })
}

fn restored_pi_log_baseline(config: &AgentConfig, is_restored: bool) -> Option<PiLogBaseline> {
    if !is_restored || config.provider != "pi" {
        return None;
    }
    let provider_session_id = config
        .resume_session
        .as_deref()
        .map(str::trim)
        .filter(|session_id| !session_id.is_empty())?;
    let session_dir = PiProvider::session_dir(&config.session_id)?;
    pi_log_baseline(&session_dir, provider_session_id)
}

fn open_pi_log_at_cursor(
    path: &std::path::Path,
    cursor: &mut PiLogCursor,
) -> Option<std::fs::File> {
    let mut file = std::fs::File::open(path).ok()?;
    let metadata = file.metadata().ok()?;
    let identity = pi_file_identity(&metadata);
    let identity_changed = cursor
        .identity
        .zip(identity)
        .is_some_and(|(before, after)| before != after);
    let boundary_changed = if cursor.boundary.is_empty() {
        false
    } else {
        let boundary_end = cursor
            .boundary_start
            .saturating_add(cursor.boundary.len() as u64);
        if metadata.len() < boundary_end {
            true
        } else {
            file.seek(std::io::SeekFrom::Start(cursor.boundary_start))
                .ok()?;
            let mut current_boundary = vec![0; cursor.boundary.len()];
            file.read_exact(&mut current_boundary).ok()?;
            current_boundary != cursor.boundary
        }
    };
    let reset = identity_changed || boundary_changed || metadata.len() < cursor.offset;
    if reset {
        cursor.offset = 0;
        cursor.boundary_start = 0;
        cursor.boundary.clear();
    }
    cursor.identity = identity;
    file.seek(std::io::SeekFrom::Start(cursor.offset)).ok()?;
    Some(file)
}

fn antigravity_watcher_conversation(
    existing: Option<String>,
    workspace_before: Option<&str>,
    discovered: Option<String>,
) -> (Option<String>, bool) {
    if existing.is_some() {
        return (existing, false);
    }

    let conversation_id = changed_workspace_conversation(workspace_before, discovered.as_deref());
    let capture_identity = conversation_id.is_some();
    (conversation_id, capture_identity)
}

#[derive(Default)]
struct OutputReadyEmitGate {
    last_emit_at: Option<std::time::Instant>,
    delayed_emit_scheduled: bool,
}

impl OutputReadyEmitGate {
    fn after_buffer_append(&mut self, now: std::time::Instant) -> OutputReadyEmitAction {
        let elapsed = self
            .last_emit_at
            .map(|last_emit_at| now.saturating_duration_since(last_emit_at));
        if elapsed.is_none_or(|elapsed| elapsed >= OUTPUT_READY_EMIT_MIN_INTERVAL) {
            self.last_emit_at = Some(now);
            self.delayed_emit_scheduled = false;
            return OutputReadyEmitAction::EmitNow;
        }

        if self.delayed_emit_scheduled {
            return OutputReadyEmitAction::Suppress;
        }

        self.delayed_emit_scheduled = true;
        OutputReadyEmitAction::ScheduleAfter(OUTPUT_READY_EMIT_MIN_INTERVAL - elapsed.unwrap())
    }

    fn finish_delayed_emit(&mut self, buffer_has_output: bool, now: std::time::Instant) -> bool {
        self.delayed_emit_scheduled = false;
        if !buffer_has_output {
            return false;
        }

        let elapsed = self
            .last_emit_at
            .map(|last_emit_at| now.saturating_duration_since(last_emit_at));
        if elapsed.is_none_or(|elapsed| elapsed >= OUTPUT_READY_EMIT_MIN_INTERVAL) {
            self.last_emit_at = Some(now);
            return true;
        }

        false
    }
}

#[derive(Debug, PartialEq, Eq)]
enum OutputReadyEmitAction {
    EmitNow,
    ScheduleAfter(std::time::Duration),
    Suppress,
}

/// Antigravity's transcript marks every planner step as `DONE`, including
/// script execution and interim progress prose. A visible compose prompt is
/// the provider's actual end-of-turn boundary. This gate observes the PTY
/// output only while the submitted turn is processing and consumes the first
/// ready prompt, so terminal redraws cannot emit duplicate completions.
#[derive(Default)]
struct AntigravityTurnCompletionGate {
    tracking_processing_turn: bool,
    output_since_turn_started: String,
}

impl AntigravityTurnCompletionGate {
    fn observe_output(&mut self, provider_name: &str, current_status: &str, output: &str) -> bool {
        if provider_name != "antigravity" || current_status != "Processing..." {
            self.reset();
            return false;
        }

        if !self.tracking_processing_turn {
            self.tracking_processing_turn = true;
            self.output_since_turn_started.clear();
        }

        self.output_since_turn_started.push_str(output);
        const MAX_PROMPT_PROBE_CHARS: usize = 32_768;
        let char_count = self.output_since_turn_started.chars().count();
        if char_count > MAX_PROMPT_PROBE_CHARS {
            self.output_since_turn_started = self
                .output_since_turn_started
                .chars()
                .skip(char_count - MAX_PROMPT_PROBE_CHARS)
                .collect();
        }

        if crate::control::antigravity_output_has_ready_prompt(&self.output_since_turn_started) {
            self.reset();
            return true;
        }

        false
    }

    fn reset(&mut self) {
        self.tracking_processing_turn = false;
        self.output_since_turn_started.clear();
    }
}

#[derive(Default)]
struct AntigravityUserTurnReceiptTracker {
    initialized: bool,
    last_step_index: Option<u64>,
}

impl AntigravityUserTurnReceiptTracker {
    /// Positions restored agents at their existing history while allowing a
    /// fresh conversation to acknowledge a user step already present by the
    /// time the watcher first observes the database.
    fn observe(&mut self, latest_step_index: Option<u64>, skip_existing: bool) -> bool {
        if !self.initialized {
            self.initialized = true;
            if skip_existing {
                self.last_step_index = latest_step_index;
                return false;
            }
        }

        let Some(latest_step_index) = latest_step_index else {
            return false;
        };
        if self
            .last_step_index
            .is_some_and(|last_step_index| latest_step_index < last_step_index)
        {
            self.last_step_index = Some(latest_step_index);
            return false;
        }
        if self
            .last_step_index
            .is_some_and(|last_step_index| latest_step_index == last_step_index)
        {
            return false;
        }

        self.last_step_index = Some(latest_step_index);
        true
    }
}

#[derive(Default)]
struct CodexTerminalThemeProbeResponder {
    answered_light_dark: bool,
    answered_foreground: bool,
    answered_background: bool,
    answered_palette_zero: bool,
    tail: Vec<u8>,
}

impl CodexTerminalThemeProbeResponder {
    fn responses_for_chunk(
        &mut self,
        provider_name: &str,
        chunk: &[u8],
        theme: &str,
    ) -> Vec<Vec<u8>> {
        if provider_name != "codex" || chunk.is_empty() {
            self.remember_tail(chunk);
            return Vec::new();
        }

        let mut data = self.tail.clone();
        data.extend_from_slice(chunk);
        let terminal_theme = CodexTerminalTheme::from_wardian_theme(theme);
        let mut responses = Vec::new();

        if !self.answered_light_dark && contains_bytes(&data, b"\x1b[?996n") {
            self.answered_light_dark = true;
            responses.push(
                format!(
                    "\x1b[?997;{}n",
                    if terminal_theme.prefers_light { 2 } else { 1 }
                )
                .into_bytes(),
            );
        }

        if !self.answered_foreground
            && (contains_bytes(&data, b"\x1b]10;?\x07")
                || contains_bytes(&data, b"\x1b]10;?\x1b\\"))
        {
            self.answered_foreground = true;
            responses.push(format!("\x1b]10;rgb:{}\x1b\\", terminal_theme.foreground).into_bytes());
        }

        if !self.answered_background
            && (contains_bytes(&data, b"\x1b]11;?\x07")
                || contains_bytes(&data, b"\x1b]11;?\x1b\\"))
        {
            self.answered_background = true;
            responses.push(format!("\x1b]11;rgb:{}\x1b\\", terminal_theme.background).into_bytes());
        }

        if !self.answered_palette_zero && contains_bytes(&data, b"\x1b]4;0;?\x07") {
            self.answered_palette_zero = true;
            responses.push(format!("\x1b]4;0;rgb:{}\x07", terminal_theme.background).into_bytes());
        }

        self.remember_tail(&data);
        responses
    }

    fn remember_tail(&mut self, data: &[u8]) {
        const MAX_TERMINAL_PROBE_TAIL: usize = 32;
        let start = data.len().saturating_sub(MAX_TERMINAL_PROBE_TAIL);
        self.tail.clear();
        self.tail.extend_from_slice(&data[start..]);
    }
}

struct CodexTerminalTheme {
    foreground: &'static str,
    background: &'static str,
    prefers_light: bool,
}

impl CodexTerminalTheme {
    fn from_wardian_theme(theme: &str) -> Self {
        if theme.trim() == "light" {
            Self {
                foreground: "11/18/27",
                background: "fc/fa/f5",
                prefers_light: true,
            }
        } else {
            Self {
                foreground: "ee/f2/ee",
                background: "02/04/02",
                prefers_light: false,
            }
        }
    }
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|candidate| candidate == needle)
}

fn codex_cleared_provider_sessions(config: &AgentConfig) -> Vec<String> {
    config.codex_config().cleared_provider_sessions
}

#[cfg(windows)]
use super::cleanup_stale_session_processes;
#[cfg(target_os = "macos")]
use super::macos_extended_path;
#[cfg(all(windows, test))]
use super::{app_process_supervisor_active, assign_pid_to_job, create_kill_on_close_job};

pub(super) fn capture_init_timestamp(
    event: &AgentEvent,
    init_timestamp: &std::sync::Arc<std::sync::Mutex<Option<String>>>,
) {
    let AgentEvent::Init { timestamp, .. } = event else {
        return;
    };
    let Some(timestamp) = timestamp else {
        return;
    };
    if let Ok(mut current) = init_timestamp.lock() {
        if current.is_none() {
            *current = Some(timestamp.clone());
        }
    }
}

pub(super) fn handle_provider_init_event(
    provider: &str,
    event: &AgentEvent,
    config: &std::sync::Arc<std::sync::Mutex<AgentConfig>>,
    init_timestamp: &std::sync::Arc<std::sync::Mutex<Option<String>>>,
) -> Result<ProviderIdentityOutcome, String> {
    let AgentEvent::Init { session_id, .. } = event else {
        return Err(format!(
            "{provider} identity validation requires an initialization event"
        ));
    };

    let outcome = {
        let mut config = config
            .lock()
            .map_err(|_| format!("{provider} session configuration is unavailable"))?;
        if matches!(provider, "codex" | "opencode" | "antigravity")
            && config
                .resume_session
                .as_deref()
                .is_none_or(|value| value.trim().is_empty())
        {
            return Err(format!(
                "{provider} initialization has no pre-bound provider identity"
            ));
        }
        apply_provider_identity(provider, &mut config, session_id)?
    };
    capture_init_timestamp(event, init_timestamp);
    Ok(outcome)
}

fn codex_status_log_session(config: &AgentConfig) -> Option<String> {
    let cleared_provider_sessions = codex_cleared_provider_sessions(config);
    let candidate = config
        .resume_session
        .clone()
        .filter(|value| !value.trim().is_empty())?;

    if codex_provider_session_is_excluded(&candidate, &cleared_provider_sessions) {
        return None;
    }
    Some(candidate)
}

fn claude_status_log_session(config: &AgentConfig) -> String {
    config
        .resume_session
        .as_deref()
        .or(config.fresh_provider_session_id.as_deref())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(config.session_id.as_str())
        .to_string()
}

fn should_cleanup_stale_session_processes_before_spawn(is_restored: bool) -> bool {
    !is_restored
}

fn pty_status_event_policy_for_provider(provider_name: &str) -> ProviderStatusEventPolicy {
    match provider_name {
        "claude" => ProviderStatusEventPolicy::PreserveActionRequiredUntilTurnCompleted,
        "codex" => ProviderStatusEventPolicy::PreserveActionRequired,
        "mock" => ProviderStatusEventPolicy::RequireTurnCompleted,
        _ => ProviderStatusEventPolicy::Normal,
    }
}

#[cfg(test)]
fn line_event_status_for_pty_provider(
    provider_name: &str,
    current_status: &str,
    event: &AgentEvent,
) -> Option<&'static str> {
    super::provider_status_from_event(
        current_status,
        event,
        pty_status_event_policy_for_provider(provider_name),
    )
}

fn persist_runtime_agent_configs(app: &AppHandle) {
    let state = app.state::<AppState>();
    let snapshot = tauri::async_runtime::block_on(async {
        let agents = state.agents.lock().await;
        let order = state.agent_order.lock().await;
        super::state_configs_snapshot(&agents, &order)
    });
    super::save_state_snapshot(app, &snapshot);
}

pub(crate) fn persist_agent_record(
    config: &AgentConfig,
    created_at: Option<&str>,
) -> Result<(), String> {
    let workspace = if config.folder.is_empty() {
        crate::utils::fs::resolve_cwd(&config.folder, &config.session_id)
            .to_string_lossy()
            .to_string()
    } else {
        config.folder.clone()
    };
    let project = wardian_core::db::project_name_from_workspace(&workspace);
    wardian_core::db::upsert_agent(&wardian_core::db::AgentUpsert {
        session_id: &config.session_id,
        session_name: &config.session_name,
        description: &config.description,
        agent_class: &config.agent_class,
        provider: &config.provider,
        workspace: Some(&workspace),
        project: project.as_deref(),
        is_off: config.is_off,
        created_at,
    })
    .map_err(|error| format!("Failed to persist agent runtime state: {error}"))
}

async fn spawn_agent_with_broker_mode(
    app: AppHandle,
    mut config: AgentConfig,
    is_restored: bool,
    initial_timestamp: Option<String>,
    stage_runtime_replacement: bool,
) -> Result<ActiveAgent, String> {
    super::validate_session_values_for_launch(
        &config.session_id,
        config.resume_session.as_deref(),
    )?;
    let provider = ProviderFactory::resolve(&config.provider)?;
    crate::providers::readiness::ensure_provider_available_for_launch(&config.provider)?;

    let cwd = crate::utils::fs::resolve_cwd(&config.folder, &config.session_id);
    let antigravity_workspace_before = if config.provider == "antigravity"
        && config
            .resume_session
            .as_deref()
            .is_none_or(|value| value.trim().is_empty())
    {
        let excluded = config.antigravity_config().cleared_conversations;
        AntigravityProvider::antigravity_home().and_then(|home| {
            AntigravityProvider::verified_conversation_for_workspace(&home, &cwd, &excluded)
        })
    } else {
        None
    };

    let expected_folder = if config.folder.is_empty() {
        cwd.to_string_lossy().to_string()
    } else {
        config.folder.clone()
    };

    let born_to_save = initial_timestamp
        .clone()
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true));

    let app_state = app.state::<AppState>();
    if config.is_off {
        if stage_runtime_replacement {
            return Err("Cannot stage an off agent runtime replacement".to_string());
        }
        let _ = persist_agent_record(&config, Some(&born_to_save));
        app_state
            .interactions
            .start_provider_input_generation(
                &config.session_id,
                ProviderInputReadiness::Unavailable,
                None,
            )
            .await;
        let _ = wardian_core::db::update_agent_status(&config.session_id, "Off", None);
        let session_id = config.session_id.clone();

        return Ok(ActiveAgent {
            config: std::sync::Arc::new(std::sync::Mutex::new(config)),
            child_process: None,
            background_processes: Vec::new(),
            memory_capability: None,
            runtime_generation: None,
            zellij_pane: None,
            process_id: None,
            query_count: std::sync::Arc::new(std::sync::Mutex::new(0)),
            init_timestamp: std::sync::Arc::new(std::sync::Mutex::new(Some(born_to_save))),
            current_status: std::sync::Arc::new(std::sync::Mutex::new("Off".to_string())),
            last_status_at: std::sync::Arc::new(std::sync::Mutex::new(None)),
            watch_state: std::sync::Arc::new(std::sync::Mutex::new(AgentWatchState::new(
                session_id, 4096, 262_144,
            ))),
            terminal_title: std::sync::Arc::new(std::sync::Mutex::new(String::new())),
            last_output_at: std::sync::Arc::new(std::sync::Mutex::new(None)),
            log_path: std::sync::Arc::new(std::sync::Mutex::new(None)),
            log_last_modified: std::sync::Arc::new(std::sync::Mutex::new(None)),
            #[cfg(windows)]
            job_object: None,
        });
    }

    app_state
        .interactions
        .start_provider_input_generation(&config.session_id, ProviderInputReadiness::Booting, None)
        .await;

    let config_lock = std::sync::Arc::new(std::sync::Mutex::new(config.clone()));

    let live_conversation_started_at =
        chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    app_state
        .conversation_archive
        .begin_live_conversation(&config.session_id, &live_conversation_started_at)
        .map_err(|error| format!("Failed to establish chat conversation boundary: {error}"))?;

    #[cfg(windows)]
    if should_cleanup_stale_session_processes_before_spawn(is_restored) {
        cleanup_stale_session_processes(&config.session_id, &config.provider);
    }

    crate::commands::terminal::log_terminal_runtime_diagnostics_once();

    let initial_geometry = app_state
        .terminal_sessions
        .spawn_geometry(&config.session_id)
        .await
        .map_err(|error| format!("Failed to read terminal spawn geometry: {error}"))?
        .unwrap_or(wardian_core::models::TerminalGeometry { cols: 80, rows: 24 });

    let (bin, mut provider_args) = provider.get_executable();
    let claude_hook = if config.provider == "claude" {
        Some(ensure_claude_permission_hook(&config.session_id)?)
    } else {
        None
    };
    let memory_process_key = if is_restored {
        config
            .resume_session
            .clone()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| config.session_id.clone())
    } else {
        format!("fresh:{}:{born_to_save}", config.session_id)
    };
    let memory_setup = match wardian_core::memory::MemoryStore::from_default_home() {
        Ok(store) => match store.compile_brief(
            &wardian_core::memory::MemoryActor::agent(&config.session_id),
            &config.session_id,
            Some(&expected_folder),
            &config.provider,
            &memory_process_key,
            is_restored,
            12_000,
        ) {
            Ok(brief) => Some((store, brief)),
            Err(error) => {
                log_debug(&format!(
                    "[Wardian] memory recall unavailable for {}: {error}",
                    config.session_id
                ));
                None
            }
        },
        Err(error) => {
            log_debug(&format!(
                "[Wardian] memory store unavailable for {}: {error}",
                config.session_id
            ));
            None
        }
    };
    let habitat_root = prepare_provider_habitat(
        &config.provider,
        &cwd,
        &config.agent_class,
        Some(&config.session_id),
    )?;
    if let Some(root) = habitat_root.as_ref() {
        crate::utils::fs::append_habitat_memory_instructions(
            root,
            memory_setup
                .as_ref()
                .and_then(|(_, brief)| (!brief.is_empty).then_some(brief.context_text.as_str())),
        )?;
        if !crate::utils::fs::provider_uses_projected_workspace(&config.provider) {
            let include = root.to_string_lossy().to_string();
            let includes = config
                .system_include_directories
                .get_or_insert_with(Vec::new);
            if !includes.contains(&include) {
                includes.push(include);
            }
        }
    }
    let provider_cwd =
        interactive_provider_cwd(&config.provider, &cwd, habitat_root.as_deref(), None);
    let fresh_claude_log_paths =
        if config.provider == "claude" && config.fresh_provider_session_id.is_some() {
            dirs::home_dir()
                .map(|home| {
                    claude_log_paths(
                        &home
                            .join(".claude")
                            .join("projects")
                            .join(claude_project_dir_name(&expected_folder)),
                    )
                })
                .unwrap_or_default()
        } else {
            std::collections::HashSet::new()
        };

    if config.provider == "claude" {
        if let Some(hook) = claude_hook.as_ref() {
            provider_args.push("--settings".to_string());
            provider_args.push(hook.settings_arg.clone());
        }
    }

    let mut background_processes = Vec::new();
    let is_resume = config
        .resume_session
        .as_deref()
        .is_some_and(|s| !s.is_empty());
    let spawn_args = provider.get_spawn_args(&config, is_resume);
    let spawn_args = finalize_interactive_spawn_args(
        &config.provider,
        is_restored,
        &config.resume_session,
        spawn_args,
    );
    provider_args.extend(spawn_args);
    if config.provider == "codex" {
        let runtime_instructions = wardian_memory_instructions(
            memory_setup
                .as_ref()
                .and_then(|(_, brief)| (!brief.is_empty).then_some(brief.context_text.as_str())),
        );
        CodexProvider::new()
            .insert_developer_instructions_arg(&mut provider_args, &runtime_instructions);
    }
    provider_args = interactive_provider_args(&config.provider, &provider_cwd, &cwd, provider_args);

    let launch_spec = interactive_provider_launch(&config.provider, &bin, &provider_args)?;
    log_debug(&format!(
        "[Wardian] PTY spawn: provider={} exe={} arg_count={} cwd={}",
        config.provider,
        launch_spec.executable,
        launch_spec.args.len(),
        provider_cwd.display()
    ));
    let mut launch_env = std::collections::BTreeMap::<String, String>::new();
    launch_env.extend(super::terminal_identity_env());
    launch_env.extend(super::interactive_provider_runtime_env(&config.provider)?);
    launch_env.insert("WARDIAN_SESSION_ID".to_string(), config.session_id.clone());
    let memory_capability = super::issue_memory_capability(&config.session_id);
    if let Some(capability) = memory_capability.as_ref() {
        launch_env.insert(
            wardian_core::memory::MEMORY_CAPABILITY_ENV.to_string(),
            capability.token().to_string(),
        );
    }
    for (key, value) in super::worktree_build_env(&config) {
        launch_env.insert(key, value);
    }

    if config.provider == "codex" {
        if let Some(root) = habitat_root.as_ref() {
            launch_env.insert(
                "CODEX_HOME".to_string(),
                habitat_codex_home(root).to_string_lossy().to_string(),
            );
        }
    } else if config.provider == "opencode" {
        for (key, value) in opencode_interactive_env(&provider_cwd, &config)? {
            launch_env.insert(key, value);
        }
    } else if config.provider == "mock" {
        let provider_session_id = expected_caller_owned_identity(&config).ok_or_else(|| {
            "mock provider launch has no caller-owned session identity".to_string()
        })?;
        launch_env.insert(
            "WARDIAN_MOCK_SESSION_ID".to_string(),
            provider_session_id.to_string(),
        );

        let mut has_config_scenario = false;
        let mut has_config_delay = false;
        if let ProviderConfig::Mock(mock) = &config.provider_config {
            if let Some(scenario) = mock.scenario.as_deref().filter(|value| !value.is_empty()) {
                launch_env.insert("WARDIAN_MOCK_SCENARIO".to_string(), scenario.to_string());
                has_config_scenario = true;
            }
            if let Some(delay_ms) = mock.delay_ms {
                launch_env.insert("WARDIAN_MOCK_DELAY_MS".to_string(), delay_ms.to_string());
                has_config_delay = true;
            }
        }
        for key in [
            "WARDIAN_MOCK_SCENARIO",
            "WARDIAN_MOCK_DELAY_MS",
            "WARDIAN_MOCK_SCRIPT",
        ] {
            if (key == "WARDIAN_MOCK_SCENARIO" && has_config_scenario)
                || (key == "WARDIAN_MOCK_DELAY_MS" && has_config_delay)
            {
                continue;
            }
            if let Ok(value) = std::env::var(key) {
                launch_env.insert(key.to_string(), value);
            }
        }

        // Mirrors the event stream to a provider log so the chat transcript can
        // read it back, matching how every real provider is observed.
        if let Some(path) = mock_transcript_log_path(&config.session_id) {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::remove_file(&path);
            launch_env.insert(
                "WARDIAN_MOCK_LOG".to_string(),
                path.to_string_lossy().to_string(),
            );
        }
    }
    #[cfg(target_os = "macos")]
    launch_env.insert("PATH".to_string(), macos_extended_path());

    log_debug(&format!(
        "[Wardian] Spawning {} agent. Session: {}, Resume: {}, Restored: {}",
        provider.name(),
        config.session_id,
        config
            .resume_session
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty()),
        is_restored
    ));

    // A restored Pi process can append its first turn immediately after spawn.
    // Capture the existing transcript boundary while the provider is still
    // unable to write, then start the watcher from that exact byte offset.
    let pi_log_baseline = restored_pi_log_baseline(&config, is_restored);

    #[cfg(not(test))]
    let child_process: Option<Box<dyn portable_pty::Child + Send>> = None;
    #[cfg(test)]
    let mut child_process: Option<Box<dyn portable_pty::Child + Send>> = None;
    #[cfg(all(windows, not(test)))]
    let job_object = None;
    #[cfg(all(windows, test))]
    let mut job_object = None;

    let (
        mut reader,
        terminal_runtime,
        process_id,
        mut zellij_pane,
        mut zellij_snapshot_frames,
        terminal_observation_source,
        mut pending_zellij_transport,
    ) = if let Some(engine) = app_state.zellij_terminal.get().cloned() {
        engine.start_attached_client().await?;
        let binding = engine
            .create_pane(crate::state::zellij_terminal::ZellijLaunchSpec {
                session_id: config.session_id.clone(),
                executable: launch_spec.executable.clone(),
                args: launch_spec.args.clone(),
                cwd: provider_cwd.clone(),
                env: launch_env.clone(),
            })
            .await?;
        let transport = engine.open_pane_transport(&binding).await?;
        let runtime = transport.runtime();
        (
            None,
            runtime,
            None,
            None,
            None,
            ProviderTerminalObservationSource::RenderedZellijFrame,
            Some(transport),
        )
    } else {
        #[cfg(not(test))]
        return Err("Bundled Zellij terminal engine is unavailable".to_string());

        #[cfg(test)]
        {
            // Unit-test compatibility for isolated AppState fixtures that do not
            // initialize bundled application resources. Production never falls
            // back to a Wardian-owned provider PTY.
            let pty_system = NativePtySystem::default();
            let pair = pty_system
                .openpty(PtySize {
                    rows: initial_geometry.rows,
                    cols: initial_geometry.cols,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .map_err(|e| format!("Failed to open pty: {e}"))?;
            let mut cmd = CommandBuilder::new(&launch_spec.executable);
            for arg in &launch_spec.args {
                cmd.arg(arg);
            }
            cmd.cwd(&provider_cwd);
            for (key, value) in &launch_env {
                cmd.env(key, value);
            }
            let child = pair
                .slave
                .spawn_command(cmd)
                .map_err(|e| format!("Failed to spawn command: {e}"))?;
            let process_id = child.process_id();

            #[cfg(windows)]
            {
                if !app_process_supervisor_active() {
                    if let Ok(job) = create_kill_on_close_job("agent fallback") {
                        if let Some(pid) = process_id {
                            if let Err(err) = assign_pid_to_job(&job, pid, "agent fallback") {
                                log_debug(&format!(
                                "[Wardian] Failed to assign session {} PID {} to fallback job: {}",
                                config.session_id, pid, err
                            ));
                            }
                        }
                        job_object = Some(job);
                    }
                }
            }

            let reader = pair
                .master
                .try_clone_reader()
                .map_err(|e| format!("Failed to get pty reader: {e}"))?;
            let mut writer = pair
                .master
                .take_writer()
                .map_err(|e| format!("Failed to get pty writer: {e}"))?;
            let pty_master: crate::state::terminal_session::SharedPtyMaster =
                std::sync::Arc::new(std::sync::Mutex::new(pair.master));
            drop(pair.slave);
            let (tx, mut rx) = tokio::sync::mpsc::channel::<
                crate::state::terminal_session::NativeTerminalWriteRequest,
            >(256);
            let runtime = crate::state::terminal_session::native_terminal_runtime(tx, pty_master);
            let sid_for_input = config.session_id.clone();
            let provider_name_for_input = config.provider.clone();
            std::thread::spawn(move || {
                while let Some(input) = rx.blocking_recv() {
                    let bytes = input.bytes;
                    if provider_name_for_input == "opencode" {
                        log_debug(&format!(
                            "[Wardian] OpenCode PTY input for session {}: {}",
                            sid_for_input,
                            debug_preview_bytes(&bytes, 128)
                        ));
                    }
                    log_terminal_trace_bytes(
                        &sid_for_input,
                        &provider_name_for_input,
                        "IN",
                        &bytes,
                    );
                    let write_result = writer
                        .write_all(&bytes)
                        .and_then(|_| writer.flush())
                        .map_err(|error| error.to_string());
                    match write_result {
                        Ok(()) => {
                            let _ = input.completion.send(Ok(()));
                        }
                        Err(error) => {
                            let _ = input.completion.send(Err(error.clone()));
                            log_terminal_trace_note(
                                &sid_for_input,
                                &provider_name_for_input,
                                &format!("PTY input write failed: {error}"),
                            );
                            break;
                        }
                    }
                }
                log_terminal_trace_note(
                    &sid_for_input,
                    &provider_name_for_input,
                    "input channel closed",
                );
            });
            child_process = Some(child);
            (
                Some(reader),
                runtime,
                process_id,
                None,
                None,
                ProviderTerminalObservationSource::RawProviderStream,
                None,
            )
        }
    };

    let terminal_runtime = match config.provider.as_str() {
        "codex" => terminal_runtime.ignore_scrollback_erase(),
        "pi" => terminal_runtime.reset_parser_on_scrollback_erase(),
        _ => terminal_runtime,
    };
    let runtime_start = if stage_runtime_replacement {
        app_state
            .terminal_sessions
            .stage_runtime_replacement(&config.session_id, terminal_runtime, initial_geometry)
            .await
    } else {
        app_state
            .terminal_sessions
            .start_or_replace_runtime(&config.session_id, terminal_runtime, initial_geometry)
            .await
    };
    let runtime_generation = match runtime_start {
        Ok(generation) => generation,
        Err(error) => {
            let message = format!("Failed to start terminal session broker: {error}");
            if let Some(mut transport) = pending_zellij_transport.take() {
                return match transport.shutdown().await {
                    Ok(()) => Err(message),
                    Err(cleanup_error) => {
                        transport.schedule_shutdown_retry();
                        Err(format!(
                            "{message}; {cleanup_error}; cleanup retry scheduled"
                        ))
                    }
                };
            }
            return Err(message);
        }
    };

    if let Some(transport) = pending_zellij_transport.take() {
        let active = transport.into_active();
        reader = Some(active.reader);
        zellij_snapshot_frames = Some(active.snapshot_frames);
        background_processes.push(active.subscription);
        zellij_pane = Some(active.lease);
    }
    let mut reader = reader.expect("terminal reader must exist after broker start");

    // Do not advertise the replacement as active in SQLite until both its
    // provider pane/transport and broker runtime exist. A failed restart must
    // leave the previous durable row intact for coherent recovery.
    let _ = persist_agent_record(&config, Some(&born_to_save));
    let _ = wardian_core::db::update_agent_status(&config.session_id, "Idle", process_id);
    let sid_out = config.session_id.clone();
    let provider_name_for_pty = config.provider.clone();
    let query_count = std::sync::Arc::new(std::sync::Mutex::new(0));
    let query_count_clone = query_count.clone();
    let init_timestamp = std::sync::Arc::new(std::sync::Mutex::new(Some(born_to_save)));
    let init_timestamp_clone = init_timestamp.clone();
    // A process can take several seconds to draw an interactive prompt. Until
    // provider-owned output (or an OpenCode title) proves that prompt exists,
    // accepting mailbox input races the provider's own startup sequence.
    let current_status = std::sync::Arc::new(std::sync::Mutex::new("Starting".to_string()));
    let current_status_clone = current_status.clone();
    let watch_state = std::sync::Arc::new(std::sync::Mutex::new(AgentWatchState::new(
        config.session_id.clone(),
        4096,
        262_144,
    )));
    let watch_state_clone = watch_state.clone();
    let terminal_title = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let terminal_title_clone = terminal_title.clone();
    let last_output_at = std::sync::Arc::new(std::sync::Mutex::new(None));
    let last_output_at_clone = last_output_at.clone();
    let initial_log_path = pi_log_baseline
        .as_ref()
        .map(|baseline| baseline.path.clone());
    let log_path = std::sync::Arc::new(std::sync::Mutex::new(initial_log_path));
    // The mock provider writes its event stream to a file it owns, so its log
    // path is known up front and needs no discovery watcher. Without this the
    // chat transcript sees nothing for a mock agent: normalized tool events
    // are only ever read back from a provider log.
    if config.provider == "mock" {
        if let Some(path) = mock_transcript_log_path(&config.session_id) {
            if let Ok(mut lock) = log_path.lock() {
                *lock = Some(path);
            }
        }
    }
    // Terminal reader thread: rendered Zellij frames update visible terminal
    // state only; the unit-test raw PTY fallback may also classify events.
    let pty_app = app.clone();
    let pty_provider = provider.clone();
    let sid_for_pty = sid_out.clone();
    let pty_emit_app = app.clone();
    let terminal_theme_for_pty = app_state.terminal_theme();
    let terminal_sessions = app_state.terminal_sessions.clone();
    let reader_runtime_generation = runtime_generation;
    let pty_config = config_lock.clone();
    let mut pending_memory_injection = memory_setup
        .map(|(store, brief)| (store, brief, expected_folder.clone(), memory_process_key));
    std::thread::spawn(move || {
        let mut buf = [0; 4096];
        let mut current_line = String::new();
        let mut had_pty_output = false;
        let mut opencode_chunks_logged = 0usize;
        let mut codex_terminal_theme_responder = CodexTerminalThemeProbeResponder::default();
        let mut antigravity_turn_completion_gate = AntigravityTurnCompletionGate::default();
        let mut opencode_startup_memory_transition = OpenCodeStartupMemoryTransition::default();
        let mut startup_prompt_pending = true;
        let mut pty_decoder = PtyUtf8Decoder::new();
        let output_ready_emit_gate =
            std::sync::Arc::new(std::sync::Mutex::new(OutputReadyEmitGate::default()));
        loop {
            match reader.read(&mut buf) {
                Ok(0) => {
                    log_terminal_trace_note(&sid_for_pty, &provider_name_for_pty, "pty EOF");
                    if provider_name_for_pty == "opencode" {
                        log_debug(&format!(
                            "[Wardian] OpenCode PTY EOF for session {} (had_output={})",
                            sid_for_pty, had_pty_output
                        ));
                    }
                    // If the process exited immediately with no output, surface a
                    // diagnostic message so the terminal is not silently blank.
                    if !had_pty_output && provider_name_for_pty == "opencode" {
                        let msg = concat!(
                            "\r\n[Wardian] OpenCode exited without producing any output.\r\n",
                            "Possible causes:\r\n",
                            "  - generated OpenCode runtime config is invalid (check ~/.wardian/wardian_debug.log)\r\n",
                            "  - OpenCode binary not found or failed to start\r\n",
                            "  - Authentication/config error in OpenCode\r\n",
                            "Check ~/.wardian/wardian_debug.log for the exact command and config used.\r\n",
                        );
                        let _ = crate::state::terminal_session::forward_terminal_output(
                            &terminal_sessions,
                            &sid_for_pty,
                            reader_runtime_generation,
                            msg.as_bytes(),
                        );
                        let _ = pty_emit_app.emit(
                            "agent-pty-output-ready",
                            serde_json::json!({ "session_id": sid_for_pty }),
                        );
                    }
                    break;
                }
                Ok(n) => {
                    if let Err(error) = crate::state::terminal_session::forward_terminal_output(
                        &terminal_sessions,
                        &sid_for_pty,
                        reader_runtime_generation,
                        &buf[..n],
                    ) {
                        log_terminal_trace_note(
                            &sid_for_pty,
                            &provider_name_for_pty,
                            &format!("broker rejected PTY reader output: {error}"),
                        );
                        break;
                    }
                    if provider_name_for_pty == "opencode" && opencode_chunks_logged < 40 {
                        log_debug(&format!(
                            "[Wardian] OpenCode PTY chunk {} for session {}: {}",
                            opencode_chunks_logged + 1,
                            sid_for_pty,
                            debug_preview_bytes(&buf[0..n], 256)
                        ));
                        opencode_chunks_logged += 1;
                    }
                    had_pty_output = true;
                    if terminal_observation_source.carries_provider_events() {
                        for response in codex_terminal_theme_responder.responses_for_chunk(
                            &provider_name_for_pty,
                            &buf[0..n],
                            &terminal_theme_for_pty,
                        ) {
                            let _ = terminal_sessions.send_privileged_input_blocking(
                                &sid_for_pty,
                                reader_runtime_generation,
                                response,
                            );
                        }
                    }
                    let latest_snapshot = zellij_snapshot_frames
                        .as_ref()
                        .and_then(|frames| frames.try_iter().last());
                    if let Ok(mut watch_state) = watch_state_clone.lock() {
                        if let Some(snapshot) = latest_snapshot.as_deref() {
                            watch_state.replace_output(snapshot);
                        } else if zellij_snapshot_frames.is_none() {
                            watch_state.push_output(&buf[0..n]);
                        }
                    }
                    log_terminal_trace_bytes(
                        &sid_for_pty,
                        &provider_name_for_pty,
                        "OUT",
                        &buf[0..n],
                    );
                    let text = pty_decoder.decode_chunk(&buf[0..n]);
                    let startup_output = if startup_prompt_pending {
                        watch_state_clone.lock().ok().and_then(|watch_state| {
                            watch_state
                                .snapshot_since(None, None)
                                .ok()
                                .map(|snapshot| snapshot.output.text)
                        })
                    } else {
                        None
                    };
                    if startup_output.as_deref().is_some_and(|output| {
                        crate::control::provider_output_has_startup_ready_prompt(
                            &provider_name_for_pty,
                            output,
                        )
                    }) {
                        startup_prompt_pending = false;
                        record_pending_memory_injection(
                            &mut pending_memory_injection,
                            &sid_for_pty,
                            &provider_name_for_pty,
                        );
                        set_agent_status(&pty_app, &sid_for_pty, &current_status_clone, "Idle");
                        let readiness_app = pty_app.clone();
                        let readiness_session_id = sid_for_pty.clone();
                        tauri::async_runtime::spawn(async move {
                            let state = readiness_app.state::<AppState>();
                            crate::control::record_provider_ready_prompt(
                                state.inner(),
                                &readiness_session_id,
                            )
                            .await;
                            crate::control::spawn_mailbox_drain_if_idle(
                                &readiness_app,
                                &readiness_session_id,
                                "Idle",
                            );
                        });
                    } else if startup_output.as_deref().is_some_and(|output| {
                        crate::control::provider_output_requires_startup_action(
                            &provider_name_for_pty,
                            output,
                        )
                    }) {
                        set_agent_status(
                            &pty_app,
                            &sid_for_pty,
                            &current_status_clone,
                            "Action Needed",
                        );
                    }
                    if terminal_observation_source
                        == ProviderTerminalObservationSource::RenderedZellijFrame
                    {
                        let provider_status = current_status_clone
                            .lock()
                            .map(|status| status.clone())
                            .unwrap_or_default();
                        if startup_prompt_pending
                            && opencode_startup_memory_transition.observe_provider_status(
                                &mut pending_memory_injection,
                                &provider_name_for_pty,
                                &provider_status,
                                &sid_for_pty,
                            )
                        {
                            startup_prompt_pending = false;
                            let readiness_app = pty_app.clone();
                            let readiness_session_id = sid_for_pty.clone();
                            tauri::async_runtime::spawn(async move {
                                let state = readiness_app.state::<AppState>();
                                crate::control::record_provider_ready_title(
                                    state.inner(),
                                    &readiness_session_id,
                                )
                                .await;
                                crate::control::spawn_mailbox_drain_if_idle(
                                    &readiness_app,
                                    &readiness_session_id,
                                    "Idle",
                                );
                            });
                        }
                    }
                    if let Ok(mut stamp) = last_output_at_clone.lock() {
                        *stamp = Some(std::time::SystemTime::now());
                    }

                    let status_before_output = current_status_clone
                        .lock()
                        .map(|status| status.clone())
                        .unwrap_or_default();
                    if observe_antigravity_terminal_completion(
                        terminal_observation_source,
                        &mut antigravity_turn_completion_gate,
                        &provider_name_for_pty,
                        &status_before_output,
                        &text,
                    ) {
                        apply_agent_event(
                            &pty_app,
                            &sid_for_pty,
                            AgentEvent::TurnCompleted,
                            &query_count_clone,
                            &init_timestamp_clone,
                            &current_status_clone,
                        );
                    }

                    if terminal_observation_source.carries_provider_events() {
                        // Raw provider streams can carry lifecycle events. A
                        // rendered Zellij frame is screen state and must never
                        // be reinterpreted as the provider's event protocol.
                        for line in text.lines() {
                            if let Some(event) = pty_provider.parse_output(line) {
                                if matches!(&event, AgentEvent::Init { .. }) {
                                    if let Err(error) = handle_provider_init_event(
                                        &provider_name_for_pty,
                                        &event,
                                        &pty_config,
                                        &init_timestamp_clone,
                                    ) {
                                        log_debug(&format!(
                                            "[WARDIAN] Rejected {} initialization identity: {}",
                                            provider_name_for_pty, error
                                        ));
                                        set_agent_status(
                                            &pty_app,
                                            &sid_for_pty,
                                            &current_status_clone,
                                            "Error",
                                        );
                                        return;
                                    }
                                }
                                apply_agent_status_event_with_policy(
                                    &pty_app,
                                    &sid_for_pty,
                                    event,
                                    &current_status_clone,
                                    pty_status_event_policy_for_provider(&provider_name_for_pty),
                                );
                            }
                        }
                    }

                    if terminal_observation_source.carries_provider_events() {
                        if let Some(title) = extract_terminal_titles(&text).into_iter().last() {
                            let _previous_title = terminal_title_clone
                                .lock()
                                .map(|value| value.clone())
                                .unwrap_or_default();
                            if provider_name_for_pty == "opencode" {
                                log_debug(&format!(
                                    "[Wardian] OpenCode backend title for session {}: {}",
                                    sid_for_pty, title
                                ));
                            }
                            if let Ok(mut current_title) = terminal_title_clone.lock() {
                                *current_title = title.clone();
                            }
                            if provider_name_for_pty == "opencode" {
                                if let Some(next_status) = opencode_startup_memory_transition
                                    .observe_title(
                                        &mut pending_memory_injection,
                                        &provider_name_for_pty,
                                        &title,
                                        &sid_for_pty,
                                    )
                                {
                                    let was_idle = current_status_clone
                                        .lock()
                                        .map(|status| {
                                            wardian_core::identity::normalize_status(&status)
                                                == "idle"
                                        })
                                        .unwrap_or(false);
                                    set_agent_status(
                                        &pty_emit_app,
                                        &sid_for_pty,
                                        &current_status_clone,
                                        next_status,
                                    );
                                    if startup_prompt_pending
                                        && opencode_startup_memory_transition.ready_observed
                                    {
                                        startup_prompt_pending = false;
                                        let readiness_app = pty_app.clone();
                                        let readiness_session_id = sid_for_pty.clone();
                                        tauri::async_runtime::spawn(async move {
                                            let state = readiness_app.state::<AppState>();
                                            crate::control::record_provider_ready_title(
                                                state.inner(),
                                                &readiness_session_id,
                                            )
                                            .await;
                                            crate::control::spawn_mailbox_drain_if_idle(
                                                &readiness_app,
                                                &readiness_session_id,
                                                "Idle",
                                            );
                                        });
                                    }
                                    // OpenCode's TUI does not expose a separate
                                    // JSON acknowledgement in interactive mode;
                                    // its provider-owned title changes from
                                    // `OpenCode` to `OC | …` when it accepts a
                                    // submitted turn.
                                    if was_idle && next_status == "Processing..." {
                                        super::emit_agent_turn_started(&pty_emit_app, &sid_for_pty);
                                    }
                                }
                            } else if provider_name_for_pty == "gemini" {
                                if let Some(next_status) = gemini_status_from_title(&title) {
                                    set_agent_status(
                                        &pty_emit_app,
                                        &sid_for_pty,
                                        &current_status_clone,
                                        next_status,
                                    );
                                }
                            }
                        }
                    }
                    let output_ready_action = output_ready_emit_gate
                        .lock()
                        .map(|mut gate| gate.after_buffer_append(std::time::Instant::now()))
                        .unwrap_or(OutputReadyEmitAction::Suppress);
                    match output_ready_action {
                        OutputReadyEmitAction::EmitNow => {
                            let _ = pty_emit_app.emit(
                                "agent-pty-output-ready",
                                serde_json::json!({ "session_id": sid_for_pty }),
                            );
                        }
                        OutputReadyEmitAction::ScheduleAfter(delay) => {
                            let delayed_app = pty_emit_app.clone();
                            let delayed_session_id = sid_for_pty.clone();
                            let delayed_gate = output_ready_emit_gate.clone();
                            tauri::async_runtime::spawn(async move {
                                tokio::time::sleep(delay).await;
                                let should_emit = delayed_gate
                                    .lock()
                                    .map(|mut gate| {
                                        gate.finish_delayed_emit(true, std::time::Instant::now())
                                    })
                                    .unwrap_or(false);
                                if should_emit {
                                    let _ = delayed_app.emit(
                                        "agent-pty-output-ready",
                                        serde_json::json!({ "session_id": delayed_session_id }),
                                    );
                                }
                            });
                        }
                        OutputReadyEmitAction::Suppress => {}
                    }
                    if terminal_observation_source.carries_provider_events() {
                        current_line.push_str(&text);
                        loop {
                            if let Some(start) = current_line.find('{') {
                                let slice = &current_line[start..];
                                let mut stream = serde_json::Deserializer::from_str(slice)
                                    .into_iter::<serde_json::Value>();
                                match stream.next() {
                                    Some(Ok(parsed)) => {
                                        // Use provider to classify the raw JSON into an AgentEvent
                                        let raw_line = parsed.to_string();
                                        if let Some(message) = extract_transcript_message(
                                            &provider_name_for_pty,
                                            &raw_line,
                                        ) {
                                            if let Ok(mut watch_state) = watch_state_clone.lock() {
                                                watch_state.push_transcript(message);
                                            }
                                        }
                                        if let Some(event) = pty_provider.parse_output(&raw_line) {
                                            if matches!(&event, AgentEvent::Init { .. }) {
                                                if let Err(error) = handle_provider_init_event(
                                                    &provider_name_for_pty,
                                                    &event,
                                                    &pty_config,
                                                    &init_timestamp_clone,
                                                ) {
                                                    log_debug(&format!(
                                                        "[WARDIAN] Rejected {} initialization identity: {}",
                                                        provider_name_for_pty, error
                                                    ));
                                                    set_agent_status(
                                                        &pty_app,
                                                        &sid_for_pty,
                                                        &current_status_clone,
                                                        "Error",
                                                    );
                                                    return;
                                                }
                                            }

                                            apply_agent_event_with_policy(
                                                &pty_app,
                                                &sid_for_pty,
                                                event,
                                                &query_count_clone,
                                                &init_timestamp_clone,
                                                &current_status_clone,
                                                pty_status_event_policy_for_provider(
                                                    &provider_name_for_pty,
                                                ),
                                            );
                                        }
                                        let _ = pty_emit_app.emit("agent-json-event", serde_json::json!({ "session_id": sid_out, "data": parsed }));
                                        let consumed = stream.byte_offset();
                                        current_line = current_line[start + consumed..].to_string();
                                        continue;
                                    }
                                    _ => break,
                                }
                            }
                            break;
                        }
                        if current_line.len() > 10000 {
                            current_line.clear();
                        }
                    }
                }
                Err(err) => {
                    log_terminal_trace_note(
                        &sid_for_pty,
                        &provider_name_for_pty,
                        &format!("pty read error: {}", err),
                    );
                    break;
                }
            }
        }
        // Process terminated (EOF or error) — mark status as Off
        set_agent_status(&pty_app, &sid_for_pty, &current_status_clone, "Off");
    });

    if config.provider == "mock" {
        let watcher_app = app.clone();
        let watcher_provider = provider.clone();
        let watcher_session = config.session_id.clone();
        let watcher_query_count = query_count.clone();
        let watcher_init_timestamp = init_timestamp.clone();
        let watcher_current_status = current_status.clone();
        let watcher_log_path = log_path.clone();
        let watcher_config = config_lock.clone();
        let watcher_watch_state = watch_state.clone();
        std::thread::spawn(move || {
            let mut offset = 0_u64;
            loop {
                let current = watcher_current_status
                    .lock()
                    .map(|status| status.clone())
                    .unwrap_or_else(|error| error.into_inner().clone());
                let path = watcher_log_path.lock().ok().and_then(|path| path.clone());
                if let Some(path) = path {
                    if let Ok(mut file) = std::fs::File::open(path) {
                        if file.seek(std::io::SeekFrom::Start(offset)).is_ok() {
                            let mut reader = std::io::BufReader::new(file);
                            let mut line = String::new();
                            loop {
                                line.clear();
                                let read = reader.read_line(&mut line).unwrap_or(0);
                                if read == 0 {
                                    break;
                                }
                                offset += read as u64;
                                let trimmed = line.trim();
                                let Ok(parsed) = serde_json::from_str::<serde_json::Value>(trimmed)
                                else {
                                    continue;
                                };
                                let raw_line = parsed.to_string();
                                if let Some(message) = extract_transcript_message("mock", &raw_line)
                                {
                                    if let Ok(mut watch_state) = watcher_watch_state.lock() {
                                        watch_state.push_transcript(message);
                                    }
                                }
                                if let Some(event) = watcher_provider.parse_output(&raw_line) {
                                    if matches!(&event, AgentEvent::Init { .. }) {
                                        if let Err(error) = handle_provider_init_event(
                                            "mock",
                                            &event,
                                            &watcher_config,
                                            &watcher_init_timestamp,
                                        ) {
                                            log_debug(&format!(
                                                "[WARDIAN] Rejected mock initialization identity: {error}"
                                            ));
                                            set_agent_status(
                                                &watcher_app,
                                                &watcher_session,
                                                &watcher_current_status,
                                                "Error",
                                            );
                                            break;
                                        }
                                    }
                                    apply_agent_event(
                                        &watcher_app,
                                        &watcher_session,
                                        event,
                                        &watcher_query_count,
                                        &watcher_init_timestamp,
                                        &watcher_current_status,
                                    );
                                }
                                let _ = watcher_app.emit(
                                    "agent-json-event",
                                    serde_json::json!({
                                        "session_id": watcher_session,
                                        "data": parsed,
                                    }),
                                );
                            }
                        }
                    }
                }
                // Drain the provider-owned log once after terminal EOF so a
                // final turn-completed event cannot be lost to watcher timing.
                if current == "Off" {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        });
    } else if config.provider == "codex" {
        let watcher_app = app.clone();
        let watcher_provider = provider.clone();
        let watcher_session = config.session_id.clone();
        let watcher_query_count = query_count.clone();
        let watcher_init_timestamp = init_timestamp.clone();
        let watcher_current_status = current_status.clone();
        let watcher_log_path = log_path.clone();
        let watcher_config = config_lock.clone();
        let watcher_watch_state = watch_state.clone();
        let watcher_skip_existing_log = is_restored;
        let wardian_agent_dir = get_wardian_home()
            .map(|home| home.join("agents").join(&watcher_session))
            .filter(|path| path.exists())
            .map(|path| path.to_string_lossy().to_string());

        std::thread::spawn(move || {
            let mut offset: u64 = 0;
            let mut last_lookup_session = String::new();
            let mut positioned_initial_log = !watcher_skip_existing_log;
            loop {
                let current = watcher_current_status
                    .lock()
                    .map(|s| s.clone())
                    .unwrap_or_else(|e| e.into_inner().clone());
                if current == "Off" {
                    break;
                }

                let path = {
                    let lookup_session = watcher_config
                        .lock()
                        .ok()
                        .and_then(|cfg| codex_status_log_session(&cfg));
                    let mut lock = watcher_log_path.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(lookup_session) = lookup_session {
                        if last_lookup_session != lookup_session {
                            *lock = None;
                            offset = 0;
                            positioned_initial_log = !watcher_skip_existing_log;
                            last_lookup_session = lookup_session.clone();
                        }
                        if lock.is_none() {
                            *lock = codex_session_file_path(
                                &lookup_session,
                                wardian_agent_dir.as_deref(),
                            );
                        }
                        lock.clone()
                    } else {
                        *lock = None;
                        offset = 0;
                        last_lookup_session.clear();
                        None
                    }
                };

                if let Some(path) = path {
                    if let Ok(mut out) = watcher_log_path.lock() {
                        *out = Some(path.clone());
                    }
                    if let Ok(mut file) = std::fs::File::open(&path) {
                        if let Ok(metadata) = file.metadata() {
                            if metadata.len() < offset {
                                offset = 0;
                            }
                            if !positioned_initial_log {
                                offset = metadata.len();
                                positioned_initial_log = true;
                            }
                        }
                        if file.seek(std::io::SeekFrom::Start(offset)).is_ok() {
                            let mut reader = std::io::BufReader::new(file);
                            let mut line = String::new();
                            loop {
                                line.clear();
                                let read = reader.read_line(&mut line).unwrap_or(0);
                                if read == 0 {
                                    break;
                                }
                                offset += read as u64;
                                if let Ok(parsed) =
                                    serde_json::from_str::<serde_json::Value>(line.trim())
                                {
                                    let raw_line = parsed.to_string();
                                    if let Some(message) =
                                        extract_transcript_message("codex", &raw_line)
                                    {
                                        if let Ok(mut watch_state) = watcher_watch_state.lock() {
                                            watch_state.push_transcript(message);
                                        }
                                    }
                                    if let Some(event) = watcher_provider.parse_output(&raw_line) {
                                        apply_agent_event_with_policy(
                                            &watcher_app,
                                            &watcher_session,
                                            event,
                                            &watcher_query_count,
                                            &watcher_init_timestamp,
                                            &watcher_current_status,
                                            pty_status_event_policy_for_provider("codex"),
                                        );
                                    }
                                    let _ = watcher_app.emit(
                                        "agent-json-event",
                                        serde_json::json!({ "session_id": watcher_session, "data": parsed }),
                                    );
                                }
                            }
                        }
                    }
                }

                std::thread::sleep(std::time::Duration::from_millis(250));
            }
        });
    } else if config.provider == "pi" {
        let watcher_app = app.clone();
        let watcher_provider = provider.clone();
        let watcher_session = config.session_id.clone();
        let watcher_config = config_lock.clone();
        let watcher_query_count = query_count.clone();
        let watcher_init_timestamp = init_timestamp.clone();
        let watcher_current_status = current_status.clone();
        let watcher_log_path = log_path.clone();
        let watcher_watch_state = watch_state.clone();
        let watcher_initial_cursor = pi_log_baseline
            .as_ref()
            .map(|baseline| baseline.cursor.clone())
            .unwrap_or_default();

        std::thread::spawn(move || {
            let mut cursor = watcher_initial_cursor;
            loop {
                let current = watcher_current_status
                    .lock()
                    .map(|status| status.clone())
                    .unwrap_or_else(|error| error.into_inner().clone());
                if current == "Off" {
                    break;
                }

                let provider_session_id = watcher_config
                    .lock()
                    .ok()
                    .and_then(|config| expected_caller_owned_identity(&config).map(str::to_string));
                let path = provider_session_id
                    .as_deref()
                    .and_then(|provider_session_id| {
                        let cached = watcher_log_path
                            .lock()
                            .ok()
                            .and_then(|path| path.clone())
                            .filter(|path| path.is_file());
                        cached.or_else(|| {
                            PiProvider::session_dir(&watcher_session).and_then(|session_dir| {
                                PiProvider::session_file(&session_dir, provider_session_id)
                            })
                        })
                    });

                if let Some(path) = path {
                    if let Ok(mut stored_path) = watcher_log_path.lock() {
                        *stored_path = Some(path.clone());
                    }
                    if let Some(file) = open_pi_log_at_cursor(&path, &mut cursor) {
                        let mut reader = std::io::BufReader::new(file);
                        let mut line = String::new();
                        loop {
                            line.clear();
                            let read = reader.read_line(&mut line).unwrap_or(0);
                            if read == 0 {
                                break;
                            }
                            cursor.offset += read as u64;
                            let trimmed = line.trim();
                            if trimmed.is_empty() {
                                continue;
                            }
                            let Ok(parsed) = serde_json::from_str::<serde_json::Value>(trimmed)
                            else {
                                continue;
                            };
                            let raw_line = parsed.to_string();
                            if let Some(message) = extract_transcript_message("pi", &raw_line) {
                                if let Ok(mut watch_state) = watcher_watch_state.lock() {
                                    watch_state.push_transcript(message);
                                }
                            }
                            if let Some(event) = watcher_provider.parse_output(&raw_line) {
                                if matches!(&event, AgentEvent::Init { .. }) {
                                    match handle_provider_init_event(
                                        "pi",
                                        &event,
                                        &watcher_config,
                                        &watcher_init_timestamp,
                                    ) {
                                        Ok(_) => {
                                            if let Ok(mut config) = watcher_config.lock() {
                                                if let Some(fresh) =
                                                    config.fresh_provider_session_id.take()
                                                {
                                                    config.resume_session = Some(fresh);
                                                }
                                            }
                                            persist_runtime_agent_configs(&watcher_app);
                                        }
                                        Err(error) => {
                                            log_debug(&format!(
                                                "[WARDIAN] Rejected Pi initialization identity: {error}"
                                            ));
                                            set_agent_status(
                                                &watcher_app,
                                                &watcher_session,
                                                &watcher_current_status,
                                                "Error",
                                            );
                                            break;
                                        }
                                    }
                                }
                                apply_agent_event(
                                    &watcher_app,
                                    &watcher_session,
                                    event,
                                    &watcher_query_count,
                                    &watcher_init_timestamp,
                                    &watcher_current_status,
                                );
                            }
                            let _ = watcher_app.emit(
                                "agent-json-event",
                                serde_json::json!({
                                    "session_id": watcher_session,
                                    "data": parsed,
                                }),
                            );
                        }
                        let mut file = reader.into_inner();
                        let _ = refresh_pi_log_boundary(&mut file, &mut cursor);
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
        });
    } else if config.provider == "claude" {
        let watcher_app = app.clone();
        let watcher_provider = provider.clone();
        let watcher_session = config.session_id.clone();
        let watcher_log_session = claude_status_log_session(&config);
        let watcher_can_capture_fresh_identity = config.fresh_provider_session_id.is_some();
        let watcher_session_name = config.session_name.clone();
        let watcher_config = config_lock.clone();
        let watcher_query_count = query_count.clone();
        let watcher_init_timestamp = init_timestamp.clone();
        let watcher_current_status = current_status.clone();
        let watcher_log_path = log_path.clone();
        let watcher_folder = expected_folder.clone();
        let watcher_fresh_claude_log_paths = fresh_claude_log_paths;
        let watcher_watch_state = watch_state.clone();
        let watcher_skip_existing_log = is_restored;
        let hook_event_log = claude_hook.as_ref().map(|hook| hook.event_log_path.clone());
        let waiting_for_permission = std::sync::Arc::new(std::sync::Mutex::new(false));
        let log_waiting_for_permission = waiting_for_permission.clone();

        std::thread::spawn(move || {
            let mut offset: u64 = 0;
            let mut positioned_initial_log = !watcher_skip_existing_log;
            loop {
                let current = watcher_current_status
                    .lock()
                    .map(|s| s.clone())
                    .unwrap_or_else(|e| e.into_inner().clone());
                if current == "Off" {
                    break;
                }

                let (path, captured_identity) = {
                    let mut lock = watcher_log_path.lock().unwrap_or_else(|e| e.into_inner());
                    let mut captured_identity = false;
                    if lock.is_none() {
                        if let Some(home) = dirs::home_dir() {
                            let project_dir = home
                                .join(".claude")
                                .join("projects")
                                .join(claude_project_dir_name(&watcher_folder));
                            let candidate =
                                project_dir.join(format!("{}.jsonl", watcher_log_session));
                            if candidate.exists()
                                && !watcher_fresh_claude_log_paths.contains(&candidate)
                            {
                                *lock = Some(candidate);
                            } else if watcher_can_capture_fresh_identity {
                                if let Some((path, provider_session_id)) =
                                    discover_claude_log_for_session_name(
                                        &project_dir,
                                        &watcher_session_name,
                                        &watcher_fresh_claude_log_paths,
                                    )
                                {
                                    if let Ok(mut cfg) = watcher_config.lock() {
                                        cfg.resume_session = Some(provider_session_id);
                                        cfg.fresh_provider_session_id = None;
                                        captured_identity = true;
                                    }
                                    *lock = Some(path);
                                }
                            }
                        }
                    }
                    (lock.clone(), captured_identity)
                };
                if captured_identity {
                    persist_runtime_agent_configs(&watcher_app);
                }

                if let Some(path) = path {
                    if let Ok(mut out) = watcher_log_path.lock() {
                        *out = Some(path.clone());
                    }
                    if let Ok(mut file) = std::fs::File::open(&path) {
                        if let Ok(metadata) = file.metadata() {
                            if metadata.len() < offset {
                                offset = 0;
                                positioned_initial_log = true;
                            }
                            if !positioned_initial_log {
                                offset = metadata.len();
                                positioned_initial_log = true;
                            }
                        }
                        if file.seek(std::io::SeekFrom::Start(offset)).is_ok() {
                            let mut reader = std::io::BufReader::new(file);
                            let mut line = String::new();
                            loop {
                                line.clear();
                                let read = reader.read_line(&mut line).unwrap_or(0);
                                if read == 0 {
                                    break;
                                }
                                offset += read as u64;
                                if let Some(message) =
                                    extract_transcript_message("claude", line.trim())
                                {
                                    if let Ok(mut watch_state) = watcher_watch_state.lock() {
                                        watch_state.push_transcript(message);
                                    }
                                }
                                if let Some(event) = watcher_provider.parse_output(line.trim()) {
                                    let mut waiting = log_waiting_for_permission
                                        .lock()
                                        .unwrap_or_else(|e| e.into_inner());
                                    if *waiting {
                                        match event {
                                            AgentEvent::UserQuery | AgentEvent::Generating => {
                                                if let Ok(parsed) =
                                                    serde_json::from_str::<serde_json::Value>(
                                                        line.trim(),
                                                    )
                                                {
                                                    let is_tool_result =
                                                        parsed.get("type").and_then(|v| v.as_str())
                                                            == Some("user")
                                                            && classify_claude_user_event(&parsed)
                                                                == ClaudeUserEventKind::ToolResult;
                                                    if is_tool_result {
                                                        *waiting = false;
                                                        apply_agent_status_event_with_policy(
                                                            &watcher_app,
                                                            &watcher_session,
                                                            event,
                                                            &watcher_current_status,
                                                            pty_status_event_policy_for_provider(
                                                                "claude",
                                                            ),
                                                        );
                                                    }
                                                }
                                            }
                                            AgentEvent::ModelResponse => {
                                                *waiting = false;
                                                apply_agent_status_event_with_policy(
                                                    &watcher_app,
                                                    &watcher_session,
                                                    event,
                                                    &watcher_current_status,
                                                    pty_status_event_policy_for_provider("claude"),
                                                );
                                            }
                                            AgentEvent::ActionRequired { .. } => {
                                                apply_agent_status_event_with_policy(
                                                    &watcher_app,
                                                    &watcher_session,
                                                    event,
                                                    &watcher_current_status,
                                                    pty_status_event_policy_for_provider("claude"),
                                                );
                                            }
                                            AgentEvent::TurnCompleted => {
                                                *waiting = false;
                                                apply_agent_status_event_with_policy(
                                                    &watcher_app,
                                                    &watcher_session,
                                                    event,
                                                    &watcher_current_status,
                                                    pty_status_event_policy_for_provider("claude"),
                                                );
                                            }
                                            AgentEvent::Init { .. } | AgentEvent::Unknown => {}
                                        }
                                    } else {
                                        apply_agent_event_with_policy(
                                            &watcher_app,
                                            &watcher_session,
                                            event,
                                            &watcher_query_count,
                                            &watcher_init_timestamp,
                                            &watcher_current_status,
                                            pty_status_event_policy_for_provider("claude"),
                                        );
                                    }
                                }
                            }
                        }
                    }
                }

                std::thread::sleep(std::time::Duration::from_millis(250));
            }
        });

        if let Some(hook_event_log) = hook_event_log {
            let hook_app = app.clone();
            let hook_session = config.session_id.clone();
            let hook_accepted_sessions = {
                let mut sessions = vec![config.session_id.clone()];
                if let Some(resume_session) = config
                    .resume_session
                    .as_ref()
                    .map(|sid| sid.trim())
                    .filter(|sid| !sid.is_empty() && *sid != config.session_id)
                {
                    sessions.push(resume_session.to_string());
                }
                if let Some(fresh_provider_session_id) = config
                    .fresh_provider_session_id
                    .as_ref()
                    .map(|sid| sid.trim())
                    .filter(|sid| !sid.is_empty() && *sid != config.session_id)
                {
                    sessions.push(fresh_provider_session_id.to_string());
                }
                sessions
            };
            let hook_current_status = current_status.clone();
            let hook_waiting_for_permission = waiting_for_permission.clone();

            std::thread::spawn(move || {
                let mut offset = 0;
                loop {
                    let current = hook_current_status
                        .lock()
                        .map(|s| s.clone())
                        .unwrap_or_else(|e| e.into_inner().clone());
                    if current == "Off" {
                        break;
                    }

                    if let Ok(mut file) = std::fs::File::open(&hook_event_log) {
                        if let Ok(metadata) = file.metadata() {
                            if metadata.len() < offset {
                                offset = 0;
                            }
                        }
                        if file.seek(std::io::SeekFrom::Start(offset)).is_ok() {
                            let mut reader = std::io::BufReader::new(file);
                            let mut line = String::new();
                            loop {
                                line.clear();
                                let read = reader.read_line(&mut line).unwrap_or(0);
                                if read == 0 {
                                    break;
                                }
                                offset += read as u64;
                                if let Ok(parsed) =
                                    serde_json::from_str::<serde_json::Value>(line.trim())
                                {
                                    if !hook_accepted_sessions.iter().any(|session_id| {
                                        claude_permission_hook_matches_session(&parsed, session_id)
                                    }) {
                                        continue;
                                    }
                                    if let Ok(mut waiting) = hook_waiting_for_permission.lock() {
                                        *waiting = true;
                                    }
                                    let tool_name = parsed
                                        .get("tool_name")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("Tool approval required")
                                        .to_string();
                                    apply_agent_status_event(
                                        &hook_app,
                                        &hook_session,
                                        AgentEvent::ActionRequired {
                                            message: tool_name.clone(),
                                        },
                                        &hook_current_status,
                                    );
                                    let _ = hook_app.emit(
                                        "agent-json-event",
                                        serde_json::json!({
                                            "session_id": hook_session,
                                            "data": {
                                                "type": "system",
                                                "subtype": "permission_request",
                                                "tool_name": tool_name,
                                            }
                                        }),
                                    );
                                }
                            }
                        }
                    }

                    std::thread::sleep(std::time::Duration::from_millis(250));
                }
            });
        }
    } else if config.provider == "antigravity" {
        let watcher_app = app.clone();
        let watcher_provider = provider.clone();
        let watcher_session = config.session_id.clone();
        let watcher_query_count = query_count.clone();
        let watcher_init_timestamp = init_timestamp.clone();
        let watcher_current_status = current_status.clone();
        let watcher_log_path = log_path.clone();
        let watcher_config = config_lock.clone();
        let watcher_watch_state = watch_state.clone();
        let watcher_skip_existing_log = is_restored;
        let watcher_workspace = cwd.clone();
        let watcher_workspace_before = antigravity_workspace_before.clone();

        std::thread::spawn(move || {
            let mut offset: u64 = 0;
            let mut positioned_initial_log = !watcher_skip_existing_log;
            let mut last_conversation_id = String::new();
            let mut user_turn_receipt_tracker = AntigravityUserTurnReceiptTracker::default();
            loop {
                let current = watcher_current_status
                    .lock()
                    .map(|s| s.clone())
                    .unwrap_or_else(|e| e.into_inner().clone());
                if current == "Off" {
                    break;
                }

                let home = AntigravityProvider::antigravity_home();
                let (conversation_id, captured_identity) = {
                    let mut cfg = watcher_config.lock().unwrap_or_else(|e| e.into_inner());
                    let existing = cfg
                        .resume_session
                        .as_ref()
                        .map(|value| value.trim().to_string())
                        .filter(|value| !value.is_empty());
                    let excluded = cfg.antigravity_config().cleared_conversations;
                    let discovered = home.as_ref().and_then(|home| {
                        AntigravityProvider::verified_conversation_for_workspace(
                            home,
                            &watcher_workspace,
                            &excluded,
                        )
                    });
                    let (conversation_id, capture_identity) = antigravity_watcher_conversation(
                        existing,
                        watcher_workspace_before.as_deref(),
                        discovered,
                    );
                    if capture_identity {
                        if let Some(conversation_id) = conversation_id.as_deref() {
                            if apply_provider_identity("antigravity", &mut cfg, conversation_id)
                                .is_ok()
                            {
                                (Some(conversation_id.to_string()), true)
                            } else {
                                (Some(conversation_id.to_string()), false)
                            }
                        } else {
                            (None, false)
                        }
                    } else {
                        (conversation_id, false)
                    }
                };
                if captured_identity {
                    persist_runtime_agent_configs(&watcher_app);
                }

                let path = conversation_id.as_deref().and_then(|conversation_id| {
                    let cached = watcher_log_path
                        .lock()
                        .ok()
                        .and_then(|path| path.clone())
                        .filter(|path| last_conversation_id == conversation_id && path.is_file());
                    cached.or_else(|| {
                        home.as_ref().and_then(|home| {
                            AntigravityProvider::conversation_log_path(home, conversation_id)
                        })
                    })
                });

                if let Some(conversation_id) = conversation_id.as_deref() {
                    if last_conversation_id != conversation_id {
                        offset = 0;
                        positioned_initial_log = !watcher_skip_existing_log;
                        user_turn_receipt_tracker = AntigravityUserTurnReceiptTracker::default();
                        last_conversation_id = conversation_id.to_string();
                    }
                }

                let database_path = if path
                    .as_ref()
                    .is_some_and(|path| path.extension().is_some_and(|extension| extension == "db"))
                {
                    path.clone()
                } else if path.is_none() {
                    conversation_id.as_deref().and_then(|conversation_id| {
                        home.as_ref().and_then(|home| {
                            let database = AntigravityProvider::conversation_database_path(
                                home,
                                conversation_id,
                            );
                            database.is_file().then_some(database)
                        })
                    })
                } else {
                    None
                };

                if let Some(database_path) = database_path {
                    if let Ok(latest_step_index) =
                        AntigravityProvider::latest_user_message_step_index(&database_path)
                    {
                        if user_turn_receipt_tracker
                            .observe(latest_step_index, watcher_skip_existing_log)
                        {
                            apply_agent_event(
                                &watcher_app,
                                &watcher_session,
                                AgentEvent::UserQuery,
                                &watcher_query_count,
                                &watcher_init_timestamp,
                                &watcher_current_status,
                            );
                        }
                    }
                }

                if let (Some(_conversation_id), Some(path)) = (conversation_id, path) {
                    if let Ok(mut out) = watcher_log_path.lock() {
                        *out = Some(path.clone());
                    }
                    // Antigravity 1.1.7 keeps interactive history in SQLite.
                    // Chat reads that database directly; the streaming watcher
                    // below remains for the legacy JSONL format.
                    if path.extension().is_some_and(|extension| extension == "db") {
                        std::thread::sleep(std::time::Duration::from_millis(250));
                        continue;
                    }
                    if let Ok(mut file) = std::fs::File::open(&path) {
                        if let Ok(metadata) = file.metadata() {
                            if metadata.len() < offset {
                                offset = 0;
                                positioned_initial_log = true;
                            }
                            if !positioned_initial_log {
                                offset = metadata.len();
                                positioned_initial_log = true;
                            }
                        }
                        if file.seek(std::io::SeekFrom::Start(offset)).is_ok() {
                            let mut reader = std::io::BufReader::new(file);
                            let mut line = String::new();
                            loop {
                                line.clear();
                                let read = reader.read_line(&mut line).unwrap_or(0);
                                if read == 0 {
                                    break;
                                }
                                offset += read as u64;
                                let trimmed = line.trim();
                                if trimmed.is_empty() {
                                    continue;
                                }
                                if let Ok(parsed) =
                                    serde_json::from_str::<serde_json::Value>(trimmed)
                                {
                                    let raw_line = parsed.to_string();
                                    if let Some(message) =
                                        extract_transcript_message("antigravity", &raw_line)
                                    {
                                        if let Ok(mut watch_state) = watcher_watch_state.lock() {
                                            watch_state.push_transcript(message);
                                        }
                                    }
                                    if let Some(event) = watcher_provider.parse_output(&raw_line) {
                                        apply_agent_event(
                                            &watcher_app,
                                            &watcher_session,
                                            event,
                                            &watcher_query_count,
                                            &watcher_init_timestamp,
                                            &watcher_current_status,
                                        );
                                    }
                                    let _ = watcher_app.emit(
                                        "agent-json-event",
                                        serde_json::json!({ "session_id": watcher_session, "data": parsed }),
                                    );
                                }
                            }
                        }
                    }
                }

                std::thread::sleep(std::time::Duration::from_millis(250));
            }
        });
    }

    // OpenCode creates a provider-owned session only once its interactive TUI
    // begins a turn. Capture that local identity instead of bootstrapping it
    // with an extra `opencode run` model request.
    if config.provider == "opencode" && config.resume_session.is_none() {
        let watcher_app = app.clone();
        let watcher_config = config_lock.clone();
        let watcher_current_status = current_status.clone();
        let watcher_workspace = cwd.clone();
        let started_after_ms = chrono::Utc::now().timestamp_millis();
        std::thread::spawn(move || loop {
            let current = watcher_current_status
                .lock()
                .map(|status| status.clone())
                .unwrap_or_default();
            if current == "Off" {
                break;
            }
            if wardian_core::identity::normalize_status(&current) == "processing" {
                if let Some(provider_session_id) =
                    opencode_recent_session_for_workspace(&watcher_workspace, started_after_ms)
                {
                    if let Ok(mut cfg) = watcher_config.lock() {
                        cfg.resume_session = Some(provider_session_id);
                        cfg.fresh_provider_session_id = None;
                    }
                    persist_runtime_agent_configs(&watcher_app);
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        });
    }

    // ── OpenCode log-file watcher ─────────────────────────────────────────
    {
        let mut cfg = config_lock.lock().unwrap();
        cfg.folder = expected_folder;
    }

    Ok(ActiveAgent {
        config: config_lock,
        child_process,
        background_processes,
        memory_capability,
        runtime_generation: Some(runtime_generation),
        zellij_pane,
        process_id,
        query_count,
        init_timestamp,
        current_status,
        last_status_at: std::sync::Arc::new(std::sync::Mutex::new(None)),
        watch_state,
        terminal_title,
        last_output_at,
        log_path,
        log_last_modified: std::sync::Arc::new(std::sync::Mutex::new(None)),
        #[cfg(windows)]
        job_object,
    })
}

pub async fn spawn_agent(
    app: AppHandle,
    config: AgentConfig,
    is_restored: bool,
    initial_timestamp: Option<String>,
) -> Result<ActiveAgent, String> {
    spawn_agent_with_broker_mode(app, config, is_restored, initial_timestamp, false).await
}

pub async fn spawn_agent_replacement(
    app: AppHandle,
    config: AgentConfig,
    is_restored: bool,
    initial_timestamp: Option<String>,
) -> Result<ActiveAgent, String> {
    spawn_agent_with_broker_mode(app, config, is_restored, initial_timestamp, true).await
}

pub async fn resize_pty(
    session_id: String,
    cols: u16,
    rows: u16,
    state: &AppState,
) -> Result<(), String> {
    if cols < 10 {
        return Ok(());
    }
    let geometry = wardian_core::models::TerminalGeometry { cols, rows };
    match state
        .terminal_sessions
        .resize_legacy(&session_id, geometry)
        .await
    {
        Ok(result)
            if result.decision.status
                == wardian_core::models::TerminalLeaseDecisionStatus::Accepted =>
        {
            Ok(())
        }
        Ok(result) => Err(format!(
            "Terminal resize lease rejected: {}",
            result
                .decision
                .reason
                .map(|reason| format!("{reason:?}"))
                .unwrap_or_else(|| "unknown".to_string())
        )),
        Err(crate::state::terminal_session::TerminalBrokerError::SessionNotFound) => {
            let agents = state.agents.lock().await;
            if !agents.contains_key(&session_id) {
                return Err(format!("Agent {} not found", session_id));
            }
            drop(agents);
            state
                .terminal_sessions
                .remember_deferred_geometry(&session_id, "legacy-resize-adapter", geometry)
                .await
                .map(|_| ())
                .map_err(|error| error.to_string())
        }
        Err(error) => Err(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wardian_core::models::{CodexProviderConfig, ProviderConfig};

    #[test]
    fn codex_status_log_session_does_not_use_latest_fallback() {
        let config = AgentConfig {
            provider: "codex".to_string(),
            resume_session: None,
            provider_config: ProviderConfig::Codex(CodexProviderConfig {
                cleared_provider_sessions: vec!["provider-session-1".to_string()],
                ..Default::default()
            }),
            ..Default::default()
        };

        let log_session = codex_status_log_session(&config);

        assert_eq!(log_session, None);
        assert_eq!(config.resume_session, None);
        assert_eq!(
            config.codex_config().cleared_provider_sessions,
            vec!["provider-session-1".to_string()]
        );
    }

    #[test]
    fn claude_status_log_session_prefers_the_provider_identity() {
        let config = AgentConfig {
            provider: "claude".to_string(),
            session_id: "wardian-session".to_string(),
            resume_session: Some("provider-session".to_string()),
            fresh_provider_session_id: Some("fresh-provider-session".to_string()),
            ..Default::default()
        };

        assert_eq!(claude_status_log_session(&config), "provider-session");

        let fresh_config = AgentConfig {
            resume_session: None,
            ..config
        };
        assert_eq!(
            claude_status_log_session(&fresh_config),
            "fresh-provider-session"
        );
    }

    #[test]
    fn restored_spawns_skip_stale_process_scan() {
        assert!(!should_cleanup_stale_session_processes_before_spawn(true));
        assert!(should_cleanup_stale_session_processes_before_spawn(false));
    }

    #[test]
    fn restored_pi_baseline_preserves_events_appended_before_first_watcher_poll() {
        let dir = tempfile::tempdir().expect("Pi session directory");
        let path = dir.path().join("session.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"type\":\"session\",\"id\":\"pi-session\"}\n",
                "{\"type\":\"message_end\",\"message\":{\"role\":\"assistant\",\"content\":\"old\",\"stopReason\":\"stop\"}}\n",
            ),
        )
        .expect("existing Pi transcript");
        let baseline = pi_log_baseline(dir.path(), "pi-session").expect("Pi baseline");

        let mut append = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("append Pi turn before watcher starts");
        writeln!(
            append,
            "{{\"type\":\"message_end\",\"message\":{{\"role\":\"assistant\",\"content\":\"new\",\"stopReason\":\"stop\"}}}}"
        )
        .expect("new Pi turn");
        drop(append);

        let mut cursor = baseline.cursor;
        let mut file = open_pi_log_at_cursor(&baseline.path, &mut cursor).expect("positioned log");
        let mut observed = String::new();
        file.read_to_string(&mut observed).expect("new Pi events");

        assert!(!observed.contains("old"));
        assert!(observed.contains("new"));
    }

    #[test]
    fn restored_pi_cursor_resets_for_larger_same_path_replacement() {
        let dir = tempfile::tempdir().expect("Pi session directory");
        let path = dir.path().join("session.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"type\":\"session\",\"id\":\"pi-session\"}\n",
                "{\"type\":\"message_end\",\"message\":{\"role\":\"assistant\",\"content\":\"old\",\"stopReason\":\"stop\"}}\n",
            ),
        )
        .expect("existing Pi transcript");
        let baseline = pi_log_baseline(dir.path(), "pi-session").expect("Pi baseline");
        let replacement = dir.path().join("replacement.jsonl");
        let new_content = format!(
            "{}{}{}",
            "{\"type\":\"session\",\"id\":\"pi-session\"}\n",
            "{\"type\":\"message_end\",\"message\":{\"role\":\"assistant\",\"content\":\"new\",\"stopReason\":\"stop\"}}\n",
            "x".repeat(baseline.cursor.offset as usize),
        );
        std::fs::write(&replacement, new_content).expect("replacement Pi transcript");
        std::fs::remove_file(&path).expect("remove old Pi transcript");
        std::fs::rename(&replacement, &path).expect("replace Pi transcript at same path");

        let mut cursor = baseline.cursor;
        let mut file = open_pi_log_at_cursor(&baseline.path, &mut cursor).expect("replacement log");
        let mut observed = String::new();
        file.read_to_string(&mut observed)
            .expect("replacement Pi events");

        assert_eq!(cursor.offset, 0);
        assert!(observed.contains("new"));
        assert!(!observed.contains("old"));
    }

    #[test]
    fn restored_pi_cursor_resets_for_in_place_rewrite_after_prefix() {
        let dir = tempfile::tempdir().expect("Pi session directory");
        let path = dir.path().join("session.jsonl");
        let padding = "x".repeat(8192);
        let old_content = format!(
            "{}{{\"type\":\"padding\",\"content\":\"{}\"}}\n{}",
            "{\"type\":\"session\",\"id\":\"pi-session\"}\n",
            padding,
            "{\"type\":\"message_end\",\"message\":{\"role\":\"assistant\",\"content\":\"old\",\"stopReason\":\"stop\"}}\n",
        );
        std::fs::write(&path, &old_content).expect("existing Pi transcript");
        let baseline = pi_log_baseline(dir.path(), "pi-session").expect("Pi baseline");
        let new_content = old_content.replace("\"content\":\"old\"", "\"content\":\"new\"");
        assert_eq!(new_content.len(), old_content.len());
        assert_eq!(&new_content[..4096], &old_content[..4096]);
        std::fs::write(&path, new_content).expect("in-place Pi transcript rewrite");

        let mut cursor = baseline.cursor;
        let mut file = open_pi_log_at_cursor(&baseline.path, &mut cursor).expect("rewritten log");
        let mut observed = String::new();
        file.read_to_string(&mut observed)
            .expect("rewritten Pi events");
        let assistant_messages = observed
            .lines()
            .filter_map(|line| extract_transcript_message("pi", line))
            .collect::<Vec<_>>();

        assert_eq!(cursor.offset, 0);
        assert!(assistant_messages
            .iter()
            .any(|message| message.text == "new"));
        assert!(!assistant_messages
            .iter()
            .any(|message| message.text == "old"));
    }

    fn agent_without_pty() -> crate::state::ActiveAgent {
        crate::state::ActiveAgent {
            config: std::sync::Arc::new(std::sync::Mutex::new(AgentConfig::default())),
            child_process: None,
            background_processes: Vec::new(),
            memory_capability: None,
            runtime_generation: None,
            zellij_pane: None,
            process_id: None,
            query_count: std::sync::Arc::new(std::sync::Mutex::new(0)),
            init_timestamp: std::sync::Arc::new(std::sync::Mutex::new(None)),
            current_status: std::sync::Arc::new(std::sync::Mutex::new("Restoring".to_string())),
            last_status_at: std::sync::Arc::new(std::sync::Mutex::new(None)),
            watch_state: std::sync::Arc::new(std::sync::Mutex::new(
                crate::state::AgentWatchState::new("restoring-agent".to_string(), 4096, 262_144),
            )),
            terminal_title: std::sync::Arc::new(std::sync::Mutex::new(String::new())),
            last_output_at: std::sync::Arc::new(std::sync::Mutex::new(None)),
            log_path: std::sync::Arc::new(std::sync::Mutex::new(None)),
            log_last_modified: std::sync::Arc::new(std::sync::Mutex::new(None)),
            #[cfg(windows)]
            job_object: None,
        }
    }

    #[test]
    fn active_agent_lease_survives_reader_lifecycle_until_runtime_drop() {
        let temp = tempfile::tempdir().unwrap();
        let store = wardian_core::memory::MemoryStore::open(temp.path().join("memory.db")).unwrap();
        let lease = store.issue_process_capability("agent-a").unwrap();
        let token = lease.token().to_string();
        let mut agent = agent_without_pty();
        agent.memory_capability = Some(lease);

        // PTY readers hold no revoker. A reader/broker exit therefore leaves
        // the provider runtime's ActiveAgent-owned authority intact.
        assert!(store.validate_capability("agent-a", &token).unwrap());
        drop(agent);
        assert!(!store.validate_capability("agent-a", &token).unwrap());
    }

    // A resize that arrives while the agent is still a "Restoring" placeholder
    // is retained by the broker and seeds the native runtime when spawn begins.
    #[tokio::test]
    async fn resize_without_pty_records_size_for_spawn() {
        let state = AppState::new();
        state
            .agents
            .lock()
            .await
            .insert("restoring-agent".to_string(), agent_without_pty());

        let result = resize_pty("restoring-agent".to_string(), 124, 30, &state).await;

        assert!(result.is_ok());
        assert_eq!(
            state
                .terminal_sessions
                .spawn_geometry("restoring-agent")
                .await
                .expect("spawn geometry"),
            Some(wardian_core::models::TerminalGeometry {
                cols: 124,
                rows: 30
            })
        );
    }

    #[tokio::test]
    async fn resize_unknown_agent_still_errors() {
        let state = AppState::new();
        let result = resize_pty("missing".to_string(), 124, 30, &state).await;
        assert!(result.is_err());
        assert_eq!(
            state
                .terminal_sessions
                .spawn_geometry("missing")
                .await
                .expect("missing geometry"),
            None
        );
    }

    #[test]
    fn codex_line_status_preserves_action_needed_until_completion() {
        assert_eq!(
            line_event_status_for_pty_provider(
                "codex",
                "Idle",
                &AgentEvent::ActionRequired {
                    message: "approve command".to_string(),
                },
            ),
            Some("Action Needed")
        );
        assert_eq!(
            line_event_status_for_pty_provider("codex", "Action Needed", &AgentEvent::Generating),
            None
        );
        assert_eq!(
            line_event_status_for_pty_provider(
                "codex",
                "Action Needed",
                &AgentEvent::TurnCompleted,
            ),
            Some("Idle")
        );
    }

    #[test]
    fn mock_line_status_waits_for_explicit_turn_completion() {
        assert_eq!(
            line_event_status_for_pty_provider("mock", "Processing...", &AgentEvent::ModelResponse),
            None
        );
        assert_eq!(
            line_event_status_for_pty_provider("mock", "Processing...", &AgentEvent::TurnCompleted),
            Some("Idle")
        );
    }

    #[test]
    fn output_ready_emit_gate_coalesces_repeats_after_throttle() {
        let mut gate = OutputReadyEmitGate::default();
        let start = std::time::Instant::now();

        assert_eq!(
            gate.after_buffer_append(start),
            OutputReadyEmitAction::EmitNow
        );
        assert_eq!(
            gate.after_buffer_append(start + OUTPUT_READY_EMIT_MIN_INTERVAL / 2),
            OutputReadyEmitAction::ScheduleAfter(OUTPUT_READY_EMIT_MIN_INTERVAL / 2)
        );
        assert_eq!(
            gate.after_buffer_append(start + OUTPUT_READY_EMIT_MIN_INTERVAL / 2),
            OutputReadyEmitAction::Suppress
        );
        assert!(gate.finish_delayed_emit(true, start + OUTPUT_READY_EMIT_MIN_INTERVAL));
    }

    #[test]
    fn antigravity_completion_gate_emits_once_for_the_ready_prompt() {
        let mut gate = AntigravityTurnCompletionGate::default();

        assert!(!gate.observe_output(
            "antigravity",
            "Processing...",
            "Running the synchronization script...\r\n",
        ));
        assert!(gate.observe_output(
            "antigravity",
            "Processing...",
            "\r\n>\r\n? for shortcuts\r\n",
        ));
        assert!(!gate.observe_output("antigravity", "Idle", "\r\n>\r\n? for shortcuts\r\n",));
    }

    #[test]
    fn antigravity_completion_gate_ignores_ready_prompt_before_processing() {
        let mut gate = AntigravityTurnCompletionGate::default();

        assert!(!gate.observe_output("antigravity", "Idle", "\r\n>\r\n? for shortcuts\r\n",));
    }

    #[test]
    fn rendered_zellij_ready_prompt_never_completes_an_antigravity_turn() {
        let mut gate = AntigravityTurnCompletionGate::default();

        assert!(!observe_antigravity_terminal_completion(
            ProviderTerminalObservationSource::RenderedZellijFrame,
            &mut gate,
            "antigravity",
            "Processing...",
            "Running the synchronization script...\r\n>\r\n? for shortcuts\r\n",
        ));
        assert!(!gate.tracking_processing_turn);
    }

    #[test]
    fn antigravity_user_turn_receipt_tracker_skips_restored_history_and_deduplicates() {
        let mut tracker = AntigravityUserTurnReceiptTracker::default();

        assert!(!tracker.observe(Some(8), true));
        assert!(!tracker.observe(Some(8), true));
        assert!(tracker.observe(Some(12), true));
        assert!(!tracker.observe(Some(12), true));
    }

    #[test]
    fn antigravity_user_turn_receipt_tracker_accepts_first_fresh_step() {
        let mut tracker = AntigravityUserTurnReceiptTracker::default();

        assert!(!tracker.observe(None, false));
        assert!(tracker.observe(Some(4), false));
        assert!(!tracker.observe(Some(4), false));
    }

    #[test]
    fn antigravity_fresh_launch_ignores_preexisting_workspace_mapping() {
        let (conversation_id, capture_identity) = antigravity_watcher_conversation(
            None,
            Some("conversation-123"),
            Some("conversation-123".to_string()),
        );

        assert_eq!(conversation_id, None);
        assert!(!capture_identity);
    }

    #[test]
    fn opencode_provider_log_readiness_records_receipt_and_enables_resume_delta() {
        let temp = tempfile::tempdir().unwrap();
        let store = wardian_core::memory::MemoryStore::open(temp.path().join("memory.db")).unwrap();
        let agent_id = "opencode-memory-agent";
        let workspace = temp.path().to_string_lossy().to_string();
        let process_key = "opencode-provider-session";
        store
            .save(
                &wardian_core::memory::MemoryActor::Operator,
                wardian_core::memory::SaveMemoryRequest {
                    agent_id: agent_id.into(),
                    workspace: Some(workspace.clone()),
                    kind: wardian_core::memory::MemoryKind::Stable,
                    text: "Initial OpenCode memory".into(),
                    evidence_excerpt: "Established before interactive startup.".into(),
                    sources: vec![],
                    idempotency_key: None,
                },
            )
            .unwrap();
        let brief = store
            .compile_brief(
                &wardian_core::memory::MemoryActor::agent(agent_id),
                agent_id,
                Some(&workspace),
                "opencode",
                process_key,
                false,
                8_000,
            )
            .unwrap();
        let mut pending = Some((store, brief, workspace.clone(), process_key.into()));

        let mut transition = OpenCodeStartupMemoryTransition::default();
        assert!(!transition.observe_provider_status(
            &mut pending,
            "opencode",
            "Processing...",
            agent_id,
        ));
        assert!(transition.observe_provider_status(&mut pending, "opencode", "Idle", agent_id,));
        assert!(transition.ready_observed);
        assert!(!record_pending_memory_injection(
            &mut pending,
            agent_id,
            "opencode"
        ));
        assert!(pending.is_none());

        let store = wardian_core::memory::MemoryStore::open(temp.path().join("memory.db")).unwrap();
        assert_eq!(
            store
                .list_events(&wardian_core::memory::MemoryActor::Operator, agent_id,)
                .unwrap()
                .into_iter()
                .filter(|event| event.action == "loaded")
                .count(),
            1
        );
        store
            .save(
                &wardian_core::memory::MemoryActor::Operator,
                wardian_core::memory::SaveMemoryRequest {
                    agent_id: agent_id.into(),
                    workspace: Some(workspace.clone()),
                    kind: wardian_core::memory::MemoryKind::Current,
                    text: "Later OpenCode memory".into(),
                    evidence_excerpt: "Established after the startup receipt.".into(),
                    sources: vec![],
                    idempotency_key: None,
                },
            )
            .unwrap();
        let resumed = store
            .compile_brief(
                &wardian_core::memory::MemoryActor::agent(agent_id),
                agent_id,
                Some(&workspace),
                "opencode",
                process_key,
                true,
                8_000,
            )
            .unwrap();
        assert_eq!(
            resumed.kind,
            wardian_core::memory::MemoryBriefKind::ResumeDelta
        );
        assert!(resumed.context_text.contains("Later OpenCode memory"));
        assert!(!resumed.context_text.contains("Initial OpenCode memory"));
    }

    #[test]
    fn rendered_zellij_frames_never_enter_the_raw_provider_event_parser() {
        assert!(ProviderTerminalObservationSource::RawProviderStream.carries_provider_events());
        assert!(!ProviderTerminalObservationSource::RenderedZellijFrame.carries_provider_events());
    }

    #[test]
    fn codex_terminal_theme_probe_responder_answers_light_theme_queries() {
        let mut responder = CodexTerminalThemeProbeResponder::default();

        let responses = responder.responses_for_chunk(
            "codex",
            b"\x1b[?996n\x1b]10;?\x1b\\\x1b]11;?\x1b\\",
            "light",
        );

        let responses: Vec<String> = responses
            .into_iter()
            .map(|response| String::from_utf8(response).expect("utf8 response"))
            .collect();
        assert_eq!(
            responses,
            vec![
                "\x1b[?997;2n".to_string(),
                "\x1b]10;rgb:11/18/27\x1b\\".to_string(),
                "\x1b]11;rgb:fc/fa/f5\x1b\\".to_string(),
            ]
        );
    }

    #[test]
    fn codex_terminal_theme_probe_responder_handles_split_background_query() {
        let mut responder = CodexTerminalThemeProbeResponder::default();

        assert!(responder
            .responses_for_chunk("codex", b"\x1b]11", "dark")
            .is_empty());
        let responses = responder.responses_for_chunk("codex", b";?\x1b\\", "dark");

        assert_eq!(responses, vec![b"\x1b]11;rgb:02/04/02\x1b\\".to_vec()]);
        assert!(responder
            .responses_for_chunk("codex", b"\x1b]11;?\x1b\\", "dark")
            .is_empty());
    }

    #[test]
    fn codex_terminal_theme_probe_responder_ignores_other_providers() {
        let mut responder = CodexTerminalThemeProbeResponder::default();

        let responses = responder.responses_for_chunk("opencode", b"\x1b]11;?\x1b\\", "light");

        assert!(responses.is_empty());
    }
}
