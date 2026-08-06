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

    /// Arguments for `prime-agent stop <agent> --json`.
    ///
    /// Prime Agent's daemon keeps a root session tree alive after its client
    /// disconnects, so a PTY teardown alone orphans a token-spending worker.
    /// `prime-agent shutdown` is deliberately not used here: it stops every
    /// agent on the machine, including ones Wardian does not own.
    pub fn stop_args(daemon_agent_id: &str) -> Vec<String> {
        vec![
            "stop".to_string(),
            daemon_agent_id.to_string(),
            "--json".to_string(),
        ]
    }

    /// Arguments for `prime-agent list --all --json`, the startup
    /// reconciliation source for detached workers.
    pub fn list_args() -> Vec<String> {
        vec![
            "list".to_string(),
            "--all".to_string(),
            "--json".to_string(),
        ]
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
    fn list_args_include_saved_agents_as_json() {
        assert_eq!(PrimeProvider::list_args(), vec!["list", "--all", "--json"]);
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
