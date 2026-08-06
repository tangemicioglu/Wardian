use wardian_core::models::provider::{AgentEvent, AgentProvider};
use wardian_core::models::AgentConfig;

/// Provider adapter for the Prime Agent CLI.
///
/// Prime Agent is a meta-provider: it selects its own model backend, so
/// [`AgentConfig::model`] carries a composite `provider/model[:thinking]` value
/// rather than a bare model id.
///
/// Unlike the other adapters, Prime Agent emits a fully structured event stream
/// under `--mode json`, so [`AgentProvider::parse_output`] needs no marker
/// scraping. It also runs each root session in a detached daemon worker, which
/// is why [`PrimeProvider::stop_args`] exists: closing the PTY only detaches the
/// client and leaves the worker running.
pub struct PrimeProvider;

/// Environment variable pointing Prime Agent at an existing Python environment
/// instead of bootstrapping `~/.prime/agent/kernel-venv` itself.
pub const KERNEL_PYTHON_ENV: &str = "PRIME_AGENT_KERNEL_PYTHON";

/// Directory name of the Wardian-managed kernel environment, kept under the
/// Wardian home so an isolated `WARDIAN_HOME` gets an isolated kernel.
const WARDIAN_KERNEL_VENV_DIR: &str = "prime-kernel-venv";

/// Interpreter path inside a virtualenv, which differs by platform.
#[cfg(target_os = "windows")]
const VENV_PYTHON_RELATIVE: &str = "Scripts/python.exe";
#[cfg(not(target_os = "windows"))]
const VENV_PYTHON_RELATIVE: &str = "bin/python";

/// Resolves the Python interpreter Prime Agent should use for its IPython
/// kernel, preferring an explicit environment override.
///
/// Prime Agent 0.7.0 cannot bootstrap its own kernel on Windows: it invokes
/// `uv pip install --python <venv>/bin/python`, the POSIX virtualenv layout,
/// while `uv venv` on Windows produces `Scripts\python.exe`. The install fails
/// and the `ipython` tool -- Prime Agent's only tool -- is unusable. Wardian
/// therefore manages the environment itself and passes it through
/// [`KERNEL_PYTHON_ENV`].
pub fn kernel_python() -> Option<std::path::PathBuf> {
    if let Some(configured) = std::env::var_os(KERNEL_PYTHON_ENV) {
        let path = std::path::PathBuf::from(configured);
        if path.is_file() {
            return Some(path);
        }
    }

    let candidate = wardian_kernel_venv_dir()?.join(VENV_PYTHON_RELATIVE);
    candidate.is_file().then_some(candidate)
}

/// Location of the Wardian-managed kernel environment, whether or not it
/// currently exists.
pub fn wardian_kernel_venv_dir() -> Option<std::path::PathBuf> {
    crate::utils::fs::get_wardian_home().map(|home| home.join(WARDIAN_KERNEL_VENV_DIR))
}

/// Session directory Wardian pins for an agent, so Prime Agent's append-only
/// JSONL transcript lands in the agent's own workspace instead of the shared
/// `~/.prime/agent/sessions` pool.
///
/// This keeps a session readable with no live provider process and makes
/// "newest JSONL in this directory" a reliable fallback when the session header
/// is missed.
pub fn session_dir_for_agent(wardian_session_id: &str) -> Option<std::path::PathBuf> {
    let trimmed = wardian_session_id.trim();
    if trimmed.is_empty() {
        return None;
    }

    crate::utils::fs::get_wardian_home()
        .map(|home| home.join("agents").join(trimmed).join("prime-sessions"))
}

/// One row of `prime-agent list --all --json`.
///
/// This is Wardian's reconciliation view of a Prime session tree, not a Wardian
/// agent: `id` is the daemon's active-session id, which is also what
/// `prime-agent stop` accepts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrimeDaemonSession {
    pub id: String,
    /// The session UUID, which is what Wardian persists in
    /// [`AgentConfig::resume_session`] and therefore the join key between a
    /// daemon row and a Wardian agent.
    pub session_id: Option<String>,
    /// The supervisor's id for a session it is actually hosting. Absent on a
    /// saved session read back from disk, which is the only reliable way to
    /// tell the two row shapes apart.
    pub active_session_id: Option<String>,
    /// Pid of the worker process, reported only for a live daemon session.
    pub worker_pid: Option<u64>,
    /// `live` for a running root, `draft` for one that has not started work.
    pub lifecycle: Option<String>,
    /// `idle`, `working`, and similar.
    pub activity: Option<String>,
    pub cwd: Option<String>,
    pub session_file: Option<String>,
    pub attached_clients: u64,
    pub message_count: u64,
    pub is_streaming: bool,
    /// `0` is a root session; deeper values are RLM descendants.
    pub rlm_depth: u64,
}

impl PrimeDaemonSession {
    fn from_value(value: &serde_json::Value) -> Option<Self> {
        let id = value.get("id")?.as_str()?.trim().to_string();
        if id.is_empty() {
            return None;
        }

        let string_field = |key: &str| {
            value
                .get(key)
                .and_then(|field| field.as_str())
                .map(str::to_string)
                .filter(|field| !field.trim().is_empty())
        };

        Some(Self {
            id,
            session_id: string_field("sessionId"),
            active_session_id: string_field("activeSessionId"),
            worker_pid: value.get("workerPid").and_then(serde_json::Value::as_u64),
            lifecycle: string_field("lifecycle"),
            activity: string_field("activity"),
            cwd: string_field("cwd"),
            session_file: string_field("sessionFile"),
            attached_clients: value
                .get("attachedClients")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0),
            message_count: value
                .get("messageCount")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0),
            is_streaming: value
                .get("isStreaming")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            rlm_depth: value
                .get("rlmDepth")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0),
        })
    }

    /// True when this row is the daemon's view of a given Wardian agent, whose
    /// provider identity is the session UUID rather than the short daemon id.
    pub fn matches_session(&self, session_uuid: &str) -> bool {
        let session_uuid = session_uuid.trim();
        !session_uuid.is_empty()
            && (self.id == session_uuid
                || self
                    .session_id
                    .as_deref()
                    .is_some_and(|value| value == session_uuid))
    }

    /// True when this is a root session tree rather than an RLM descendant.
    pub fn is_root(&self) -> bool {
        self.rlm_depth == 0
    }

    /// True when the supervisor is actually hosting this session, as opposed
    /// to it being a saved session read back from disk.
    ///
    /// This distinction is not cosmetic. A saved row still reports
    /// `lifecycle: "live"` and `attachedClients: 0` long after its worker
    /// exited, so lifecycle alone would classify every finished session as a
    /// running one. Only a hosted session carries `activeSessionId`, and a
    /// hosted session with a worker process also reports `workerPid`.
    pub fn is_hosted_by_daemon(&self) -> bool {
        self.active_session_id.is_some()
    }

    /// True when the worker is running with no client attached, which is the
    /// state Wardian shows as detached after an app restart.
    pub fn is_detached(&self) -> bool {
        self.is_hosted_by_daemon()
            && self.attached_clients == 0
            && self
                .lifecycle
                .as_deref()
                .is_some_and(|lifecycle| lifecycle.eq_ignore_ascii_case("live"))
    }
}

/// One Prime worker's session tree: a root and the subagents under it.
///
/// This is a projection for display, not a Wardian agent. Only the root
/// corresponds to something the user created; the subagents are Prime's own
/// `rlm` children and are read-only.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrimeSessionTree {
    pub worker_pid: u64,
    /// Absent when the listing shows subagents whose root has already
    /// finished, which is possible mid-teardown.
    pub root: Option<PrimeDaemonSession>,
    pub subagents: Vec<PrimeDaemonSession>,
}

impl PrimeSessionTree {
    /// True when Prime is running subagents under this root right now.
    pub fn has_subagents(&self) -> bool {
        !self.subagents.is_empty()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PrimeRunSummary {
    pub session_id: Option<String>,
    pub last_text: Option<String>,
}

impl Default for PrimeProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl PrimeProvider {
    pub fn new() -> Self {
        PrimeProvider
    }

    #[cfg(not(target_os = "windows"))]
    fn find_unix_prime_in_paths<I>(paths: I) -> Option<String>
    where
        I: IntoIterator<Item = std::path::PathBuf>,
    {
        for path in paths {
            let full_path = path.join("prime-agent");
            if full_path.exists() {
                return Some(full_path.to_string_lossy().to_string());
            }
        }

        None
    }

    /// Arguments for `prime-agent stop <selector> --json`.
    ///
    /// Prime Agent's daemon keeps a root session tree alive after its client
    /// disconnects, so a PTY teardown alone orphans a token-spending worker.
    /// `prime-agent shutdown` is deliberately not used here: it stops every
    /// agent on the machine, including ones Wardian does not own.
    ///
    /// The selector is resolved by the supervisor against the short daemon id,
    /// the session UUID, and the session name, so Wardian's persisted
    /// `resume_session` is a first-class target and no extra id lookup is
    /// needed. Only workers with no owning client are visible to this command;
    /// that is the interactive lifecycle Wardian spawns, while `--print` and
    /// `--mode rpc` clients own their worker and take it down themselves.
    pub fn stop_args(selector: &str) -> Vec<String> {
        vec![
            "stop".to_string(),
            selector.to_string(),
            "--json".to_string(),
        ]
    }

    /// True when a failed launch was rejected by Prime's session lease rather
    /// than by anything about the request itself.
    ///
    /// Prime guards each session file with a lease keyed by the owning process.
    /// A resume issued while the previous worker is still shutting down is
    /// refused with `SessionAlreadyActiveError`, observed as
    /// `Session is already active in <agent>: <path>` (or
    /// `... in another process: <path>` when the owner is unnamed).
    pub fn is_session_lease_conflict(error: &str) -> bool {
        error.contains("Session is already active")
            || error.contains("session_already_active")
            || error.contains("SessionAlreadyActiveError")
    }

    /// The agent named as holding the lease, when the message names one.
    ///
    /// Note that this id is not necessarily stoppable: the lease outlives its
    /// worker, so the supervisor can answer `Unknown active session` for an id
    /// that a lease conflict just reported. Treat it as a diagnostic.
    pub fn session_lease_conflict_owner(error: &str) -> Option<String> {
        let owner = error
            .split_once("Session is already active in ")?
            .1
            .split_once(':')?
            .0
            .trim();

        (!owner.is_empty() && owner != "another process").then(|| owner.to_string())
    }

    /// Backoff schedule for retrying a launch refused by the session lease.
    ///
    /// Prime reclaims a lease as soon as it observes the owning process is
    /// gone, comparing both liveness and process start id; there is no timed
    /// grace period to wait out. The retry window is therefore only as long as
    /// the previous worker takes to exit, so the schedule stays short and gives
    /// up quickly rather than masking a genuinely still-running agent.
    pub const SESSION_LEASE_RETRY_BACKOFF: [std::time::Duration; 3] = [
        std::time::Duration::from_millis(250),
        std::time::Duration::from_millis(750),
        std::time::Duration::from_millis(2000),
    ];

    /// Arguments for `prime-agent list --json`, the startup reconciliation
    /// source for detached workers.
    ///
    /// `--all` is deliberately omitted. It adds saved sessions read back from
    /// disk, and those rows still carry `lifecycle: "live"` with
    /// `attachedClients: 0` long after their worker exited. Including them
    /// would make every finished session look like a running one.
    pub fn list_args() -> Vec<String> {
        vec!["list".to_string(), "--json".to_string()]
    }

    /// Appends the model selector. Prime accepts `provider/id` directly, so a
    /// composite value is passed through untouched and a bare id is sent as-is
    /// for Prime's own pattern matching to resolve.
    fn append_model_args(args: &mut Vec<String>, config: &AgentConfig) {
        if let Some(model) = config.model.as_ref().filter(|s| !s.trim().is_empty()) {
            args.push("--model".into());
            args.push(model.trim().to_string());
        }

        let prime = config.prime_config();
        if let Some(thinking) = prime.thinking.as_ref().filter(|s| !s.trim().is_empty()) {
            args.push("--thinking".into());
            args.push(thinking.trim().to_string());
        }
    }

    /// Appends tool, extension, and skill selection shared by interactive and
    /// headless launches.
    fn append_resource_args(args: &mut Vec<String>, config: &AgentConfig) {
        let prime = config.prime_config();

        if !prime.tools.is_empty() {
            args.push("--tools".into());
            args.push(prime.tools.join(","));
        }
        if prime.no_builtin_tools.unwrap_or(false) {
            args.push("--no-builtin-tools".into());
        }
        for extension in prime.extensions.iter().filter(|s| !s.trim().is_empty()) {
            args.push("--extension".into());
            args.push(extension.trim().to_string());
        }
        for skill in prime.skills.iter().filter(|s| !s.trim().is_empty()) {
            args.push("--skill".into());
            args.push(skill.trim().to_string());
        }
    }

    /// Appends autonomous-mode budgets and completion gates.
    ///
    /// Supplying any `--autonomous-*` sub-option also enables autonomous mode,
    /// so the explicit `--autonomous` flag is only emitted when no gates or
    /// budgets are configured.
    pub fn append_autonomous_args(args: &mut Vec<String>, config: &AgentConfig) {
        let prime = config.prime_config();
        if !prime.autonomous.unwrap_or(false) {
            return;
        }

        args.push("--autonomous".into());
        for gate in prime.autonomous_gates.iter().filter(|s| !s.trim().is_empty()) {
            args.push("--autonomous-gate".into());
            args.push(gate.trim().to_string());
        }
        if let Some(max_turns) = prime.autonomous_max_turns {
            args.push("--autonomous-max-turns".into());
            args.push(max_turns.to_string());
        }
        if let Some(max_tokens) = prime.autonomous_max_tokens {
            args.push("--autonomous-max-tokens".into());
            args.push(max_tokens.to_string());
        }
    }

    /// Parses `prime-agent list --all --json` into the fields Wardian needs to
    /// reconcile detached workers after a restart.
    ///
    /// Prime keeps root sessions alive in daemon workers after their client
    /// disconnects, so this is the only way Wardian can rediscover agents it
    /// started in a previous run.
    pub fn parse_list_output(output: &str) -> Result<Vec<PrimeDaemonSession>, String> {
        let parsed: serde_json::Value = serde_json::from_str(output.trim())
            .map_err(|error| format!("Prime Agent returned unreadable session JSON: {error}"))?;

        let sessions = parsed
            .get("sessions")
            .and_then(|value| value.as_array())
            .ok_or_else(|| "Prime Agent session listing had no `sessions` array".to_string())?;

        Ok(sessions
            .iter()
            .filter_map(PrimeDaemonSession::from_value)
            .collect())
    }

    /// Selects the persisted agents whose Prime worker is still alive with no
    /// client attached, which is the state a Wardian restart has to recover.
    ///
    /// `agents` pairs each Wardian session id with its persisted provider
    /// session, normally `AgentConfig::resume_session`. An agent with no
    /// provider session was never bound to a worker and cannot be reconciled.
    /// RLM descendants are skipped: they are projections of a root tree, not
    /// agents Wardian owns, so adopting one would create a duplicate.
    pub fn detached_agent_sessions<'a>(
        sessions: &[PrimeDaemonSession],
        agents: impl IntoIterator<Item = (&'a str, Option<&'a str>)>,
    ) -> Vec<(String, PrimeDaemonSession)> {
        agents
            .into_iter()
            .filter_map(|(wardian_session_id, provider_session)| {
                let provider_session = provider_session?.trim();
                if provider_session.is_empty() {
                    return None;
                }
                let matched = sessions.iter().find(|session| {
                    session.is_root()
                        && session.is_detached()
                        && session.matches_session(provider_session)
                })?;
                Some((wardian_session_id.to_string(), matched.clone()))
            })
            .collect()
    }

    /// Groups hosted sessions into root trees for read-only projection.
    ///
    /// Prime runs one worker per root session tree, so every row reporting the
    /// same `workerPid` belongs to one tree: the `rlmDepth == 0` row is the
    /// root and the deeper rows are the subagents it spawned through `rlm`.
    /// That grouping is the only parentage the listing exposes -- rows carry
    /// no parent pointer -- so a tree deeper than one level is reported flat,
    /// with each subagent's own depth preserved.
    ///
    /// Rows with no `workerPid` are skipped: without a worker there is no tree
    /// to attribute them to.
    pub fn group_session_trees(sessions: &[PrimeDaemonSession]) -> Vec<PrimeSessionTree> {
        let mut trees: Vec<PrimeSessionTree> = Vec::new();

        for session in sessions.iter().filter(|s| s.is_hosted_by_daemon()) {
            let Some(worker_pid) = session.worker_pid else {
                continue;
            };

            let tree = match trees.iter_mut().find(|tree| tree.worker_pid == worker_pid) {
                Some(existing) => existing,
                None => {
                    trees.push(PrimeSessionTree {
                        worker_pid,
                        root: None,
                        subagents: Vec::new(),
                    });
                    trees.last_mut().expect("just pushed")
                }
            };

            if session.is_root() && tree.root.is_none() {
                tree.root = Some(session.clone());
            } else {
                tree.subagents.push(session.clone());
            }
        }

        // Shallowest first, so a projection renders parents before children
        // even though the listing does not order them.
        for tree in &mut trees {
            tree.subagents.sort_by_key(|session| session.rlm_depth);
        }

        trees
    }

    /// Extracts the session id and final assistant text from a completed
    /// `--mode json` run.
    pub fn summarize_run_output(output: &str) -> PrimeRunSummary {
        let mut summary = PrimeRunSummary::default();

        for line in output.lines() {
            let Ok(parsed) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };

            match parsed.get("type").and_then(|value| value.as_str()) {
                Some("session") => {
                    if summary.session_id.is_none() {
                        summary.session_id = parsed
                            .get("id")
                            .and_then(|value| value.as_str())
                            .map(|value| value.to_string());
                    }
                }
                Some("message_end") => {
                    if let Some(text) = assistant_message_text(parsed.get("message")) {
                        summary.last_text = Some(text);
                    }
                }
                _ => {}
            }
        }

        summary
    }
}

/// Concatenates the text blocks of an assistant message, ignoring tool calls
/// and reasoning blocks.
fn assistant_message_text(message: Option<&serde_json::Value>) -> Option<String> {
    let message = message?;
    if message.get("role").and_then(|value| value.as_str()) != Some("assistant") {
        return None;
    }

    let text = message
        .get("content")?
        .as_array()?
        .iter()
        .filter(|block| block.get("type").and_then(|value| value.as_str()) == Some("text"))
        .filter_map(|block| block.get("text").and_then(|value| value.as_str()))
        .collect::<Vec<_>>()
        .join("");

    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

impl AgentProvider for PrimeProvider {
    fn name(&self) -> &str {
        "Prime Agent"
    }

    fn get_executable(&self) -> (String, Vec<String>) {
        #[cfg(target_os = "windows")]
        {
            if let Some(paths) = std::env::var_os("PATH") {
                let path_exts = std::env::var("PATHEXT")
                    .ok()
                    .map(|value| {
                        value
                            .split(';')
                            .filter_map(|segment| {
                                let trimmed = segment.trim();
                                if trimmed.is_empty() {
                                    None
                                } else {
                                    Some(trimmed.to_ascii_lowercase())
                                }
                            })
                            .collect::<Vec<_>>()
                    })
                    .filter(|exts| !exts.is_empty())
                    .unwrap_or_else(|| {
                        vec![".exe".to_string(), ".cmd".to_string(), ".bat".to_string()]
                    });

                for path in std::env::split_paths(&paths) {
                    if let Some(launch) =
                        crate::providers::npm::node_launch_from_npm_cmd_shim(&path, "prime-agent")
                    {
                        return launch;
                    }

                    for ext in &path_exts {
                        let candidate = path.join(format!("prime-agent{ext}"));
                        if candidate.exists() {
                            return (candidate.to_string_lossy().to_string(), vec![]);
                        }
                    }
                }
            }

            // The Prime Agent installer is a wrapper around `npm install -g`,
            // so the shim lands in the npm global prefix even when the Wardian
            // app environment has a narrower PATH than the user's shell.
            if let Some(appdata) = dirs::data_dir() {
                let npm_dir = appdata.join("npm");
                if let Some(launch) =
                    crate::providers::npm::node_launch_from_npm_cmd_shim(&npm_dir, "prime-agent")
                {
                    return launch;
                }

                let npm_shim = npm_dir.join("prime-agent.cmd");
                if npm_shim.exists() {
                    return (npm_shim.to_string_lossy().to_string(), vec![]);
                }
            }

            ("prime-agent".to_string(), vec![])
        }

        #[cfg(not(target_os = "windows"))]
        {
            if let Some(paths) = std::env::var_os("PATH") {
                if let Some(executable) =
                    Self::find_unix_prime_in_paths(std::env::split_paths(&paths))
                {
                    return (executable, vec![]);
                }
            }

            let home = dirs::home_dir().unwrap_or_default();
            let fallbacks = vec![
                home.join(".npm-global/bin/prime-agent"),
                std::path::PathBuf::from("/usr/local/bin/prime-agent"),
                std::path::PathBuf::from("/opt/homebrew/bin/prime-agent"),
            ];
            for path in fallbacks {
                if path.exists() {
                    return (path.to_string_lossy().to_string(), vec![]);
                }
            }

            ("prime-agent".to_string(), vec![])
        }
    }

    fn get_spawn_args(&self, config: &AgentConfig, is_resume: bool) -> Vec<String> {
        let mut args = Vec::new();
        let prime = config.prime_config();

        Self::append_model_args(&mut args, config);
        Self::append_resource_args(&mut args, config);

        if let Some(session_dir) =
            session_dir_for_agent(&config.session_id).filter(|_| !config.session_id.trim().is_empty())
        {
            args.push("--session-dir".into());
            args.push(session_dir.to_string_lossy().to_string());
        }

        if is_resume {
            if let Some(session_id) = config
                .resume_session
                .as_ref()
                .filter(|s| !s.trim().is_empty())
            {
                args.push("--resume".into());
                args.push(session_id.trim().to_string());
            }
        } else if let Some(goal) = prime.goal.as_ref().filter(|s| !s.trim().is_empty()) {
            // --goal only seeds a new root session with no existing goal state.
            args.push("--goal".into());
            args.push(goal.trim().to_string());
        }

        if let Some(custom) = config.custom_args.as_ref() {
            if let Some(parsed) = shlex::split(custom) {
                args.extend(parsed);
            }
        }

        args
    }

    fn parse_output(&self, line: &str) -> Option<AgentEvent> {
        let parsed: serde_json::Value = serde_json::from_str(line).ok()?;
        let msg_type = parsed.get("type")?.as_str()?;

        match msg_type {
            // The session header is the first line of every run and carries the
            // provider session id, so Prime needs no bootstrap handshake.
            "session" => {
                let session_id = parsed.get("id")?.as_str()?.to_string();
                let timestamp = parsed
                    .get("timestamp")
                    .and_then(|value| value.as_str())
                    .map(|value| value.to_string());
                Some(AgentEvent::Init {
                    session_id,
                    timestamp,
                })
            }
            "turn_start" => Some(AgentEvent::UserQuery),
            "agent_end" => Some(AgentEvent::TurnCompleted),
            // The only event where Prime is waiting on a person. Non-blocking
            // methods such as `notify` and `setStatus` reach the same event
            // type and must not stall the agent, so the method decides.
            "extension_ui_request" => {
                let request = crate::providers::prime_rpc::parse_extension_ui_request(line)
                    .filter(|request| request.blocks_the_agent());
                Some(match request {
                    Some(request) => AgentEvent::ActionRequired {
                        message: request.prompt_text(),
                    },
                    None => AgentEvent::Generating,
                })
            }
            "agent_start"
            | "turn_end"
            | "message_start"
            | "message_update"
            | "message_end"
            | "tool_execution_start"
            | "tool_execution_update"
            | "tool_execution_end"
            | "compaction_start"
            | "compaction_end"
            | "auto_retry_start"
            | "auto_retry_end" => Some(AgentEvent::Generating),
            _ => Some(AgentEvent::Unknown),
        }
    }

    fn get_instruction_filename(&self) -> &str {
        "AGENTS.md"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wardian_core::models::{PrimeProviderConfig, ProviderConfig};

    fn make_provider() -> PrimeProvider {
        PrimeProvider::new()
    }

    fn make_prime_config(prime: PrimeProviderConfig) -> AgentConfig {
        AgentConfig {
            provider: "prime".into(),
            provider_config: ProviderConfig::Prime(prime),
            ..Default::default()
        }
    }

    #[test]
    fn name_returns_prime_agent() {
        assert_eq!(make_provider().name(), "Prime Agent");
    }

    #[test]
    fn instruction_filename_is_agents_md() {
        assert_eq!(make_provider().get_instruction_filename(), "AGENTS.md");
    }

    #[test]
    fn spawn_args_pass_composite_model_and_thinking() {
        let config = AgentConfig {
            model: Some("anthropic/claude-opus-5".into()),
            ..make_prime_config(PrimeProviderConfig {
                thinking: Some("high".into()),
                ..Default::default()
            })
        };

        let args = make_provider().get_spawn_args(&config, false);

        assert_eq!(
            args,
            vec![
                "--model",
                "anthropic/claude-opus-5",
                "--thinking",
                "high"
            ]
        );
    }

    #[test]
    fn spawn_args_include_tools_extensions_and_skills() {
        let config = make_prime_config(PrimeProviderConfig {
            tools: vec!["ipython".into()],
            no_builtin_tools: Some(true),
            extensions: vec!["./wardian-extension".into()],
            skills: vec!["C:/skills/review".into()],
            ..Default::default()
        });

        let args = make_provider().get_spawn_args(&config, false);

        assert!(args.windows(2).any(|w| w == ["--tools", "ipython"]));
        assert!(args.contains(&"--no-builtin-tools".to_string()));
        assert!(args
            .windows(2)
            .any(|w| w == ["--extension", "./wardian-extension"]));
        assert!(args.windows(2).any(|w| w == ["--skill", "C:/skills/review"]));
    }

    #[test]
    fn spawn_args_resume_uses_resume_flag_and_drops_goal() {
        let config = AgentConfig {
            resume_session: Some("019fd48e-0fcd-73cf-8039-f1eed51c5123".into()),
            ..make_prime_config(PrimeProviderConfig {
                goal: Some("ship the release".into()),
                ..Default::default()
            })
        };

        let args = make_provider().get_spawn_args(&config, true);

        assert!(args
            .windows(2)
            .any(|w| w == ["--resume", "019fd48e-0fcd-73cf-8039-f1eed51c5123"]));
        assert!(!args.contains(&"--goal".to_string()));
    }

    #[test]
    fn spawn_args_seed_goal_only_for_new_sessions() {
        let config = make_prime_config(PrimeProviderConfig {
            goal: Some("ship the release".into()),
            ..Default::default()
        });

        let args = make_provider().get_spawn_args(&config, false);

        assert!(args.windows(2).any(|w| w == ["--goal", "ship the release"]));
    }

    #[test]
    fn spawn_args_parse_custom_args() {
        let config = AgentConfig {
            custom_args: Some("--offline --verbose".into()),
            ..make_prime_config(PrimeProviderConfig::default())
        };

        let args = make_provider().get_spawn_args(&config, false);

        assert!(args.contains(&"--offline".to_string()));
        assert!(args.contains(&"--verbose".to_string()));
    }

    #[test]
    fn autonomous_args_are_omitted_when_disabled() {
        let config = make_prime_config(PrimeProviderConfig {
            autonomous_gates: vec!["npm run lint".into()],
            ..Default::default()
        });

        let mut args = Vec::new();
        PrimeProvider::append_autonomous_args(&mut args, &config);

        assert!(args.is_empty());
    }

    #[test]
    fn autonomous_args_include_gates_and_budgets() {
        let config = make_prime_config(PrimeProviderConfig {
            autonomous: Some(true),
            autonomous_gates: vec!["npm run lint".into(), "cargo clippy".into()],
            autonomous_max_turns: Some(12),
            autonomous_max_tokens: Some(80000),
            ..Default::default()
        });

        let mut args = Vec::new();
        PrimeProvider::append_autonomous_args(&mut args, &config);

        assert!(args.contains(&"--autonomous".to_string()));
        assert_eq!(
            args.iter().filter(|a| *a == "--autonomous-gate").count(),
            2
        );
        assert!(args.windows(2).any(|w| w == ["--autonomous-gate", "npm run lint"]));
        assert!(args.windows(2).any(|w| w == ["--autonomous-max-turns", "12"]));
        assert!(args.windows(2).any(|w| w == ["--autonomous-max-tokens", "80000"]));
    }

    #[test]
    fn stop_args_target_a_single_agent() {
        assert_eq!(
            PrimeProvider::stop_args("6e65660fc3ea"),
            vec!["stop", "6e65660fc3ea", "--json"]
        );
    }

    #[test]
    fn a_blocking_dialog_is_the_only_action_required_signal() {
        let provider = make_provider();

        // The dialog's own text is carried through so the user sees what is
        // being asked instead of a bare "action required".
        assert!(matches!(
            provider.parse_output(
                r#"{"type":"extension_ui_request","id":"1","method":"confirm","title":"Overwrite?"}"#
            ),
            Some(AgentEvent::ActionRequired { message }) if message == "Overwrite?"
        ));
        // A notification is fire-and-forget; treating it as action required
        // would leave a working agent looking stuck.
        assert!(matches!(
            provider.parse_output(
                r#"{"type":"extension_ui_request","id":"2","method":"notify","message":"done"}"#
            ),
            Some(AgentEvent::Generating)
        ));
    }

    #[test]
    fn session_lease_conflicts_are_recognized_and_attributed() {
        // Both message forms come from SessionAlreadyActiveError in
        // prime-agent 0.7.0's core/session-lease.js.
        let named = "Session is already active in 3a87eadc7fe1: C:\\s\\019f.jsonl";
        let unnamed = "Session is already active in another process: C:\\s\\019f.jsonl";

        assert!(PrimeProvider::is_session_lease_conflict(named));
        assert!(PrimeProvider::is_session_lease_conflict(unnamed));
        assert_eq!(
            PrimeProvider::session_lease_conflict_owner(named).as_deref(),
            Some("3a87eadc7fe1")
        );
        // The unnamed form has no agent to report, only a path.
        assert_eq!(PrimeProvider::session_lease_conflict_owner(unnamed), None);
    }

    #[test]
    fn ordinary_failures_are_not_treated_as_lease_conflicts() {
        // Retrying these would only delay a real error reaching the user.
        assert!(!PrimeProvider::is_session_lease_conflict(
            "Unknown active session: 3a87eadc7fe1"
        ));
        assert!(!PrimeProvider::is_session_lease_conflict(
            "Headless provider prime exited with status 1"
        ));
        assert!(!PrimeProvider::is_session_lease_conflict(""));
    }

    #[test]
    fn lease_retry_backoff_gives_up_quickly() {
        let total: std::time::Duration = PrimeProvider::SESSION_LEASE_RETRY_BACKOFF.iter().sum();

        // The wait exists to outlast a worker's exit, not to sit through a
        // genuinely busy agent, so the whole schedule stays inside a few
        // seconds and is strictly increasing.
        assert!(total < std::time::Duration::from_secs(5));
        assert!(PrimeProvider::SESSION_LEASE_RETRY_BACKOFF
            .windows(2)
            .all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn list_args_ask_only_for_sessions_the_daemon_hosts() {
        assert_eq!(PrimeProvider::list_args(), vec!["list", "--json"]);
        assert!(!PrimeProvider::list_args().contains(&"--all".to_string()));
    }

    #[test]
    fn a_saved_session_is_not_mistaken_for_a_running_one() {
        // Captured verbatim from `prime-agent list --all --json` for a session
        // whose worker had already exited. It still says lifecycle "live" with
        // no attached clients, so lifecycle alone would call it detached and
        // Wardian would show a dead agent as running.
        let saved = r#"{"sessions":[{
          "id": "019fd4c3-8276-7368-b8b6-a9392b53ea7d",
          "lifecycle": "live",
          "activity": "idle",
          "isSessionActive": false,
          "sessionId": "019fd4c3-8276-7368-b8b6-a9392b53ea7d",
          "cwd": "C:\\Users\\t",
          "isStreaming": false,
          "attachedClients": 0,
          "messageCount": 2,
          "rlmDepth": 0
        }]}"#;

        let session = PrimeProvider::parse_list_output(saved).expect("parse").remove(0);

        assert!(!session.is_hosted_by_daemon());
        assert!(!session.is_detached());
        assert!(PrimeProvider::detached_agent_sessions(
            &[session],
            [("agent-1", Some("019fd4c3-8276-7368-b8b6-a9392b53ea7d"))]
        )
        .is_empty());
    }

    #[test]
    fn a_hosted_session_reports_its_worker() {
        // Captured verbatim from `prime-agent list --json`, which returns only
        // sessions the supervisor is hosting.
        let hosted = r#"{"sessions":[{
          "id": "b32e30bfde83",
          "lifecycle": "live",
          "runtimeKind": "top-level",
          "activeSessionId": "b32e30bfde83",
          "sessionId": "019fd4d1-0000-7000-8000-000000000000",
          "attachedClients": 1,
          "workerPid": 81900,
          "rlmDepth": 0
        }]}"#;

        let session = PrimeProvider::parse_list_output(hosted).expect("parse").remove(0);

        assert!(session.is_hosted_by_daemon());
        assert_eq!(session.worker_pid, Some(81900));
        // A client is attached, so it is being driven, not detached.
        assert!(!session.is_detached());
    }

    #[test]
    fn parse_list_output_reads_reconciliation_fields() {
        // Row captured verbatim from `prime-agent 0.7.0 list --all --json`,
        // trimmed to the fields Wardian reads. Note that `id` is a short daemon
        // id while `sessionId` is the UUID Wardian persists: they are different
        // values for the same worker.
        let output = r#"{
          "sessions": [
            {
              "id": "99dd42ff3d92",
              "lifecycle": "live",
              "activity": "idle",
              "isSessionActive": false,
              "rlmDepth": 0,
              "activeSessionId": "99dd42ff3d92",
              "sessionId": "019fd4c3-8276-7368-b8b6-a9392b53ea7d",
              "sessionFile": "C:\\Users\\t\\.prime\\agent\\sessions\\019fd4c3-8276-7368-b8b6-a9392b53ea7d.jsonl",
              "cwd": "C:\\work",
              "isStreaming": false,
              "attachedClients": 0,
              "messageCount": 4
            }
          ]
        }"#;

        let sessions = PrimeProvider::parse_list_output(output).expect("parse");

        assert_eq!(sessions.len(), 1);
        let session = &sessions[0];
        assert_eq!(session.id, "99dd42ff3d92");
        assert_eq!(
            session.session_id.as_deref(),
            Some("019fd4c3-8276-7368-b8b6-a9392b53ea7d")
        );
        assert_eq!(session.message_count, 4);
        assert!(session.is_root());
        assert!(session.is_detached());
    }

    #[test]
    fn daemon_rows_match_either_identifier_the_supervisor_accepts() {
        let session = PrimeProvider::parse_list_output(
            r#"{"sessions":[{"id":"99dd42ff3d92","sessionId":"019fd4c3-8276-7368-b8b6-a9392b53ea7d"}]}"#,
        )
        .expect("parse")
        .remove(0);

        // Wardian persists the UUID, so that is the join key that matters.
        assert!(session.matches_session("019fd4c3-8276-7368-b8b6-a9392b53ea7d"));
        assert!(session.matches_session("99dd42ff3d92"));
        assert!(!session.matches_session("019fd4c3-8276-7368-b8b6-000000000000"));
        assert!(!session.matches_session("   "));
    }

    #[test]
    fn parse_list_output_classifies_attachment_and_depth() {
        // activeSessionId is present on every row the supervisor hosts; a row
        // without it is a saved session and is covered separately.
        let output = r#"{"sessions":[
          {"id":"root-attached","activeSessionId":"root-attached","lifecycle":"live","attachedClients":1,"rlmDepth":0},
          {"id":"root-detached","activeSessionId":"root-detached","lifecycle":"live","attachedClients":0,"rlmDepth":0},
          {"id":"draft","activeSessionId":"draft","lifecycle":"draft","attachedClients":0,"rlmDepth":0},
          {"id":"child","activeSessionId":"child","lifecycle":"live","attachedClients":0,"rlmDepth":2}
        ]}"#;

        let sessions = PrimeProvider::parse_list_output(output).expect("parse");
        let by_id = |id: &str| sessions.iter().find(|s| s.id == id).expect("session").clone();

        // A client is attached, so this is an ordinary running agent.
        assert!(!by_id("root-attached").is_detached());
        // Live with no client is the state a Wardian restart must recover.
        assert!(by_id("root-detached").is_detached());
        // A draft has not started work and is not a detached worker.
        assert!(!by_id("draft").is_detached());
        // RLM descendants are projections of a root, never reconciled as agents.
        assert!(!by_id("child").is_root());
    }

    #[test]
    fn reconciliation_adopts_only_live_unattended_roots() {
        let sessions = PrimeProvider::parse_list_output(
            r#"{"sessions":[
              {"id":"a1","activeSessionId":"a1","sessionId":"uuid-detached","lifecycle":"live","attachedClients":0,"rlmDepth":0},
              {"id":"a2","activeSessionId":"a2","sessionId":"uuid-attached","lifecycle":"live","attachedClients":1,"rlmDepth":0},
              {"id":"a3","activeSessionId":"a3","sessionId":"uuid-draft","lifecycle":"draft","attachedClients":0,"rlmDepth":0},
              {"id":"a4","activeSessionId":"a4","sessionId":"uuid-child","lifecycle":"live","attachedClients":0,"rlmDepth":1}
            ]}"#,
        )
        .expect("parse");

        let adopted = PrimeProvider::detached_agent_sessions(
            &sessions,
            [
                ("agent-detached", Some("uuid-detached")),
                // Already attached, so the running app owns it.
                ("agent-attached", Some("uuid-attached")),
                // Never started work.
                ("agent-draft", Some("uuid-draft")),
                // An RLM descendant is a projection of a root, not an agent.
                ("agent-child", Some("uuid-child")),
                // Never bound to a worker.
                ("agent-unbound", None),
                ("agent-blank", Some("   ")),
                // Bound to a worker that is gone.
                ("agent-missing", Some("uuid-vanished")),
            ],
        );

        assert_eq!(adopted.len(), 1);
        assert_eq!(adopted[0].0, "agent-detached");
        assert_eq!(adopted[0].1.id, "a1");
    }

    #[test]
    fn reconciliation_matches_agents_persisted_with_the_short_daemon_id() {
        let sessions = PrimeProvider::parse_list_output(
            r#"{"sessions":[{"id":"99dd42ff3d92","activeSessionId":"99dd42ff3d92","sessionId":"uuid-1","lifecycle":"live","attachedClients":0,"rlmDepth":0}]}"#,
        )
        .expect("parse");

        let adopted =
            PrimeProvider::detached_agent_sessions(&sessions, [("agent-1", Some("99dd42ff3d92"))]);

        assert_eq!(adopted.len(), 1);
    }

    #[test]
    fn subagents_group_under_the_root_sharing_their_worker() {
        // One worker per root tree, so workerPid is the grouping key; the
        // listing carries no parent pointer of any kind.
        let sessions = PrimeProvider::parse_list_output(
            r#"{"sessions":[
              {"id":"r1","activeSessionId":"r1","workerPid":100,"rlmDepth":0,"lifecycle":"live","attachedClients":1},
              {"id":"c2","activeSessionId":"c2","workerPid":100,"rlmDepth":2,"lifecycle":"live","attachedClients":0},
              {"id":"c1","activeSessionId":"c1","workerPid":100,"rlmDepth":1,"lifecycle":"live","attachedClients":0},
              {"id":"r2","activeSessionId":"r2","workerPid":200,"rlmDepth":0,"lifecycle":"live","attachedClients":1}
            ]}"#,
        )
        .expect("parse");

        let trees = PrimeProvider::group_session_trees(&sessions);

        assert_eq!(trees.len(), 2);
        let first = trees.iter().find(|t| t.worker_pid == 100).expect("tree");
        assert_eq!(first.root.as_ref().expect("root").id, "r1");
        // Shallowest first, since the listing does not order them.
        assert_eq!(
            first
                .subagents
                .iter()
                .map(|s| s.id.as_str())
                .collect::<Vec<_>>(),
            vec!["c1", "c2"]
        );

        let second = trees.iter().find(|t| t.worker_pid == 200).expect("tree");
        assert!(!second.has_subagents());
    }

    #[test]
    fn rows_with_no_worker_are_left_out_of_every_tree() {
        // A saved session has no worker and no tree to belong to; attributing
        // it to one would invent a subagent that does not exist.
        let sessions = PrimeProvider::parse_list_output(
            r#"{"sessions":[
              {"id":"saved","sessionId":"saved","rlmDepth":0,"lifecycle":"live"},
              {"id":"hosted","activeSessionId":"hosted","rlmDepth":0,"lifecycle":"live"}
            ]}"#,
        )
        .expect("parse");

        // "hosted" is hosted but reports no workerPid, so it has no tree
        // either; only rows with a worker are grouped.
        assert!(PrimeProvider::group_session_trees(&sessions).is_empty());
    }

    #[test]
    fn parse_list_output_rejects_unusable_payloads() {
        assert!(PrimeProvider::parse_list_output("not json").is_err());
        assert!(PrimeProvider::parse_list_output(r#"{"agents":[]}"#).is_err());
        // Rows without a usable id cannot be stopped, so they are dropped.
        assert!(PrimeProvider::parse_list_output(r#"{"sessions":[{"id":"  "},{}]}"#)
            .expect("parse")
            .is_empty());
    }

    // The event fixtures below are verbatim lines captured from
    // `prime-agent 0.7.0 --mode json`.

    #[test]
    fn parse_output_session_header_is_init() {
        let line = r#"{"type":"session","version":3,"id":"019fd48e-0fcd-73cf-8039-f1eed51c5123","timestamp":"2026-08-06T00:51:47.789Z","cwd":"C:\\tmp","rlmDepth":0}"#;

        assert_eq!(
            make_provider().parse_output(line).unwrap(),
            AgentEvent::Init {
                session_id: "019fd48e-0fcd-73cf-8039-f1eed51c5123".into(),
                timestamp: Some("2026-08-06T00:51:47.789Z".into()),
            }
        );
    }

    #[test]
    fn parse_output_turn_start_is_user_query() {
        assert_eq!(
            make_provider().parse_output(r#"{"type":"turn_start"}"#).unwrap(),
            AgentEvent::UserQuery
        );
    }

    #[test]
    fn parse_output_agent_end_is_turn_completed() {
        assert_eq!(
            make_provider()
                .parse_output(r#"{"type":"agent_end","messages":[]}"#)
                .unwrap(),
            AgentEvent::TurnCompleted
        );
    }

    #[test]
    fn parse_output_turn_end_stays_generating() {
        // Prime emits turn_end per turn; only agent_end ends the run, so a
        // mid-run turn_end must not complete the Wardian turn.
        assert_eq!(
            make_provider()
                .parse_output(r#"{"type":"turn_end","message":{},"toolResults":[]}"#)
                .unwrap(),
            AgentEvent::Generating
        );
    }

    #[test]
    fn parse_output_tool_execution_is_generating() {
        let line = r#"{"type":"tool_execution_start","toolCallId":"tool:1","toolName":"ipython","args":{"code":"print(1)"}}"#;
        assert_eq!(
            make_provider().parse_output(line).unwrap(),
            AgentEvent::Generating
        );
    }

    #[test]
    fn parse_output_invalid_json_is_none() {
        assert!(make_provider().parse_output("not json").is_none());
    }

    #[test]
    fn parse_output_unknown_type_is_unknown() {
        assert_eq!(
            make_provider()
                .parse_output(r#"{"type":"session_action_update","actions":{}}"#)
                .unwrap(),
            AgentEvent::Unknown
        );
    }

    #[test]
    fn summarize_run_output_extracts_session_and_final_assistant_text() {
        let output = concat!(
            r#"{"type":"session","version":3,"id":"019fd48e-0fcd","timestamp":"t","cwd":"C:\\tmp","rlmDepth":0}"#,
            "\n",
            r#"{"type":"message_end","message":{"role":"user","content":[{"type":"text","text":"say hello"}]}}"#,
            "\n",
            r#"{"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":"Checking."},{"type":"toolCall","id":"t1","name":"ipython","arguments":{}}]}}"#,
            "\n",
            r#"{"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":"Done: wardian-spike-ok"}]}}"#,
            "\n",
        );

        let summary = PrimeProvider::summarize_run_output(output);

        assert_eq!(
            summary,
            PrimeRunSummary {
                session_id: Some("019fd48e-0fcd".into()),
                last_text: Some("Done: wardian-spike-ok".into()),
            }
        );
    }

    #[test]
    fn summarize_run_output_ignores_tool_result_and_invalid_lines() {
        let output = concat!(
            "not-json\n",
            r#"{"type":"message_end","message":{"role":"toolResult","toolCallId":"t1","content":[{"type":"text","text":"stdout"}]}}"#,
            "\n",
            r#"{"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":"final"}]}}"#,
            "\n",
        );

        let summary = PrimeProvider::summarize_run_output(output);

        assert_eq!(summary.last_text, Some("final".into()));
        assert_eq!(summary.session_id, None);
    }
}
