use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};
use wardian_core::models::chat::AgentChatRole;
use wardian_core::models::provider::{AgentEvent, AgentProvider};
use wardian_core::models::AgentConfig;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AntigravityTranscriptSummary {
    pub conversation_id: Option<String>,
    pub last_text: Option<String>,
    pub last_step_index: Option<u64>,
}

pub struct AntigravityProvider;

/// A user or model message stored in Antigravity's current SQLite
/// conversation format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AntigravityConversationMessage {
    pub step_index: u64,
    pub role: AgentChatRole,
    pub text: String,
}

pub(crate) fn changed_workspace_conversation(
    before: Option<&str>,
    after: Option<&str>,
) -> Option<String> {
    let after = after.map(str::trim).filter(|value| !value.is_empty())?;
    (before.map(str::trim) != Some(after)).then(|| after.to_string())
}

impl Default for AntigravityProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl AntigravityProvider {
    pub fn new() -> Self {
        AntigravityProvider
    }

    pub fn antigravity_home() -> Option<PathBuf> {
        dirs::home_dir().map(|home| home.join(".gemini").join("antigravity-cli"))
    }

    pub fn transcript_path(home: &Path, conversation_id: &str) -> PathBuf {
        home.join("brain")
            .join(conversation_id)
            .join(".system_generated")
            .join("logs")
            .join("transcript.jsonl")
    }

    pub fn conversation_database_path(home: &Path, conversation_id: &str) -> PathBuf {
        home.join("conversations")
            .join(format!("{conversation_id}.db"))
    }

    pub fn conversation_for_workspace(home: &Path, workspace: &Path) -> Option<String> {
        let cache = home.join("cache").join("last_conversations.json");
        let content = std::fs::read_to_string(cache).ok()?;
        let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;
        let object = parsed.as_object()?;
        let workspace_key = normalize_path_key(workspace);
        object.iter().find_map(|(key, value)| {
            (normalize_path_text(key) == workspace_key)
                .then(|| value.as_str().map(str::to_string))
                .flatten()
        })
    }

    /// Resolves only the cache entry that Antigravity explicitly associates
    /// with this workspace. Newer Antigravity builds also record the workspace
    /// URI in conversation metadata; require that match when it is available.
    /// There is intentionally no newest-file fallback here.
    pub fn verified_conversation_for_workspace(
        home: &Path,
        workspace: &Path,
        excluded_conversations: &[String],
    ) -> Option<String> {
        let conversation_id = Self::conversation_for_workspace(home, workspace)?;
        if excluded_conversations
            .iter()
            .any(|excluded| excluded.trim() == conversation_id)
        {
            return None;
        }

        match conversation_metadata_matches_workspace(home, &conversation_id, workspace) {
            Some(true) => {}
            Some(false) => return None,
            // Older CLI builds do not have metadata. Their JSONL transcript is
            // the only compatible evidence we can accept.
            None if !Self::transcript_path(home, &conversation_id).is_file() => return None,
            None => {}
        }

        Self::conversation_log_path(home, &conversation_id).map(|_| conversation_id)
    }

    /// Returns the durable log for one verified provider conversation. Version
    /// 1.1.7 stores interactive turns in SQLite and leaves the legacy JSONL
    /// transcript empty, so prefer a database that actually contains messages.
    pub fn conversation_log_path(home: &Path, conversation_id: &str) -> Option<PathBuf> {
        let database = Self::conversation_database_path(home, conversation_id);
        if Self::conversation_messages_from_database(&database)
            .map(|messages| !messages.is_empty())
            .unwrap_or(false)
        {
            return Some(database);
        }

        let transcript = Self::transcript_path(home, conversation_id);
        transcript.is_file().then_some(transcript)
    }

    pub fn conversation_messages_from_database(
        path: &Path,
    ) -> Result<Vec<AntigravityConversationMessage>, String> {
        let connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|error| format!("failed to open Antigravity conversation database: {error}"))?;
        let mut statement = connection
            .prepare("SELECT idx, step_type, step_payload FROM steps ORDER BY idx")
            .map_err(|error| format!("failed to read Antigravity conversation steps: {error}"))?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                ))
            })
            .map_err(|error| format!("failed to query Antigravity conversation steps: {error}"))?;

        let mut messages = Vec::new();
        for row in rows {
            let (step_index, step_type, payload) = row.map_err(|error| {
                format!("failed to decode Antigravity conversation step: {error}")
            })?;
            let (role, text) = match step_type {
                // USER_MESSAGE: payload.user_message.text
                14 => (
                    AgentChatRole::User,
                    protobuf_string_at_path(&payload, &[19, 2]),
                ),
                // PLANNER_RESPONSE: prefer the final response; progress prose
                // is the only visible response for intermediate planner steps.
                15 => (
                    AgentChatRole::Assistant,
                    protobuf_string_at_path(&payload, &[20, 1])
                        .or_else(|| protobuf_string_at_path(&payload, &[20, 3])),
                ),
                _ => continue,
            };
            let Some(text) = text
                .map(|text| text.trim().to_string())
                .filter(|text| !text.is_empty())
            else {
                continue;
            };
            messages.push(AntigravityConversationMessage {
                step_index: step_index.max(0) as u64,
                role,
                text,
            });
        }

        Ok(messages)
    }

    pub fn summarize_conversation(
        home: &Path,
        conversation_id: &str,
    ) -> Result<AntigravityTranscriptSummary, String> {
        let path = Self::transcript_path(home, conversation_id);
        let content = std::fs::read_to_string(&path).map_err(|err| {
            format!(
                "Failed to read Antigravity transcript {}: {}",
                path.display(),
                err
            )
        })?;
        let mut summary = Self::summarize_transcript_content(&content);
        summary.conversation_id = Some(conversation_id.to_string());
        Ok(summary)
    }

    pub fn summarize_transcript_content(content: &str) -> AntigravityTranscriptSummary {
        let mut summary = AntigravityTranscriptSummary::default();
        for line in content.lines() {
            let Ok(parsed) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            if is_antigravity_model_response(&parsed) {
                if let Some(text) = parsed.get("content").and_then(|value| value.as_str()) {
                    let trimmed = text.trim();
                    if !trimmed.is_empty() {
                        summary.last_text = Some(trimmed.to_string());
                    }
                }
                summary.last_step_index = parsed.get("step_index").and_then(|value| value.as_u64());
            }
        }
        summary
    }
}

fn conversation_metadata_matches_workspace(
    home: &Path,
    conversation_id: &str,
    workspace: &Path,
) -> Option<bool> {
    let metadata =
        std::fs::read_to_string(home.join("cache").join("conversation_metadata.json")).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&metadata).ok()?;
    let workspace_uris = parsed
        .get("conversations")?
        .get(conversation_id)?
        .get("summary")?
        .get("WorkspaceURIs")?
        .as_array()?;
    let workspace_key = normalize_path_key(workspace);
    Some(workspace_uris.iter().any(|value| {
        value
            .as_str()
            .and_then(file_uri_path_text)
            .is_some_and(|path| normalize_path_text(path) == workspace_key)
    }))
}

fn file_uri_path_text(uri: &str) -> Option<&str> {
    let path = uri.strip_prefix("file://")?;
    if path.len() >= 3 && path.starts_with('/') && path.as_bytes()[2] == b':' {
        Some(&path[1..])
    } else {
        Some(path)
    }
}

fn protobuf_string_at_path(bytes: &[u8], fields: &[u32]) -> Option<String> {
    let mut current = bytes;
    for field in fields {
        current = protobuf_length_delimited_field(current, *field)?;
    }
    String::from_utf8(current.to_vec()).ok()
}

fn protobuf_length_delimited_field(bytes: &[u8], wanted_field: u32) -> Option<&[u8]> {
    let mut offset = 0;
    while offset < bytes.len() {
        let key = protobuf_varint(bytes, &mut offset)?;
        let field = (key >> 3) as u32;
        match key & 0x07 {
            0 => {
                protobuf_varint(bytes, &mut offset)?;
            }
            1 => offset = offset.checked_add(8)?,
            2 => {
                let length = usize::try_from(protobuf_varint(bytes, &mut offset)?).ok()?;
                let end = offset.checked_add(length)?;
                let value = bytes.get(offset..end)?;
                offset = end;
                if field == wanted_field {
                    return Some(value);
                }
            }
            5 => offset = offset.checked_add(4)?,
            _ => return None,
        }
    }
    None
}

fn protobuf_varint(bytes: &[u8], offset: &mut usize) -> Option<u64> {
    let mut value = 0_u64;
    for index in 0..10 {
        let byte = *bytes.get(*offset)?;
        *offset += 1;
        let shift = index * 7;
        if shift >= 64 || (shift == 63 && (byte & 0x7f) > 1) {
            return None;
        }
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

impl AgentProvider for AntigravityProvider {
    fn name(&self) -> &str {
        "antigravity"
    }

    fn get_executable(&self) -> (String, Vec<String>) {
        #[cfg(target_os = "windows")]
        {
            if let Some(paths) = std::env::var_os("PATH") {
                for path in std::env::split_paths(&paths) {
                    for name in ["agy.exe", "agy.cmd", "agy.bat", "agy"] {
                        let candidate = path.join(name);
                        if candidate.is_file() {
                            if !name.eq_ignore_ascii_case("agy.exe") {
                                if let Some(launch) =
                                    crate::providers::npm::node_launch_from_npm_cmd_shim(
                                        &path, "agy",
                                    )
                                {
                                    return launch;
                                }
                            }
                            return (candidate.to_string_lossy().to_string(), vec![]);
                        }
                    }
                }
            }

            if let Some(local) = dirs::data_local_dir() {
                let candidate = local.join("agy").join("bin").join("agy.exe");
                if candidate.is_file() {
                    return (candidate.to_string_lossy().to_string(), vec![]);
                }
            }

            ("agy".to_string(), vec![])
        }

        #[cfg(not(target_os = "windows"))]
        {
            if let Some(paths) = std::env::var_os("PATH") {
                for path in std::env::split_paths(&paths) {
                    let candidate = path.join("agy");
                    if candidate.is_file() {
                        return (candidate.to_string_lossy().to_string(), vec![]);
                    }
                }
            }

            let home = dirs::home_dir().unwrap_or_default();
            for path in [
                home.join(".local/bin/agy"),
                PathBuf::from("/usr/local/bin/agy"),
                PathBuf::from("/opt/homebrew/bin/agy"),
            ] {
                if path.is_file() {
                    return (path.to_string_lossy().to_string(), vec![]);
                }
            }

            ("agy".to_string(), vec![])
        }
    }

    fn get_spawn_args(&self, config: &AgentConfig, is_resume: bool) -> Vec<String> {
        let mut args = Vec::new();
        let antigravity = config.antigravity_config();

        let mut directories = config
            .system_include_directories
            .clone()
            .unwrap_or_default();
        if let Some(user_dirs) = config.include_directories.as_ref() {
            for dir in user_dirs {
                if !directories.contains(dir) {
                    directories.push(dir.clone());
                }
            }
        }
        let directories = crate::utils::fs::project_antigravity_include_directories(
            &config.session_id,
            directories,
        );
        for dir in directories {
            args.push("--add-dir".to_string());
            args.push(dir);
        }

        if antigravity.sandbox.unwrap_or(false) {
            args.push("--sandbox".to_string());
        }
        if antigravity.dangerously_skip_permissions.unwrap_or(false) {
            args.push("--dangerously-skip-permissions".to_string());
        }
        if let Some(model) = config
            .model
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            args.push("--model".to_string());
            args.push(model.to_string());
        }
        if let Some(effort) = antigravity
            .reasoning_effort
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            args.push("--effort".to_string());
            args.push(effort.to_string());
        }

        if is_resume {
            if let Some(session_id) = config
                .resume_session
                .as_ref()
                .filter(|value| !value.trim().is_empty())
            {
                args.push("--conversation".to_string());
                args.push(session_id.clone());
            }
        }

        if let Some(custom) = config.custom_args.as_ref() {
            if let Some(parsed) = shlex::split(custom) {
                args.extend(parsed);
            }
        }

        args
    }

    fn parse_output(&self, line: &str) -> Option<AgentEvent> {
        let trimmed = line.trim();
        if trimmed.contains("Do you trust the contents of this project?")
            || trimmed.contains("requires permission to read, edit, and execute files here")
            || trimmed.contains("Requesting permission for:")
        {
            return Some(AgentEvent::ActionRequired {
                message: "Antigravity permission required".to_string(),
            });
        }

        let parsed: serde_json::Value = serde_json::from_str(line).ok()?;
        match parsed.get("type").and_then(|value| value.as_str()) {
            Some("USER_INPUT") => Some(AgentEvent::UserQuery),
            // Antigravity emits a DONE PLANNER_RESPONSE for every planner step:
            // tool calls, progress prose, and the final answer. The terminal
            // ready prompt is the only reliable end-of-turn boundary.
            Some("PLANNER_RESPONSE") => Some(AgentEvent::Unknown),
            Some("SYSTEM_MESSAGE") | Some("CONVERSATION_HISTORY") => Some(AgentEvent::Unknown),
            _ => Some(AgentEvent::Unknown),
        }
    }

    fn get_instruction_filename(&self) -> &str {
        "AGENTS.md"
    }
}

fn is_antigravity_model_response(value: &serde_json::Value) -> bool {
    value.get("source").and_then(|value| value.as_str()) == Some("MODEL")
        && value.get("type").and_then(|value| value.as_str()) == Some("PLANNER_RESPONSE")
        && value.get("status").and_then(|value| value.as_str()) == Some("DONE")
}

fn normalize_path_key(path: &Path) -> String {
    normalize_path_text(&path.to_string_lossy())
}

fn normalize_path_text(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    let normalized = normalized
        .strip_prefix("//?/UNC/")
        .map(|path| format!("//{path}"))
        .unwrap_or(normalized);
    normalized
        .strip_prefix("//?/")
        .unwrap_or(&normalized)
        .trim_end_matches('/')
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    #[test]
    fn bootstrap_requires_a_changed_workspace_mapping() {
        assert_eq!(
            changed_workspace_conversation(Some("old"), Some("new")).as_deref(),
            Some("new")
        );
        assert_eq!(
            changed_workspace_conversation(Some("same"), Some("same")),
            None
        );
        assert_eq!(changed_workspace_conversation(None, None), None);
    }
    use wardian_core::models::{AntigravityProviderConfig, ProviderConfig};

    fn make_provider() -> AntigravityProvider {
        AntigravityProvider::new()
    }

    fn make_antigravity_config(antigravity: AntigravityProviderConfig) -> AgentConfig {
        AgentConfig {
            provider: "antigravity".into(),
            provider_config: ProviderConfig::Antigravity(antigravity),
            ..Default::default()
        }
    }

    #[test]
    fn name_returns_lowercase_antigravity() {
        assert_eq!(make_provider().name(), "antigravity");
    }

    #[test]
    fn instruction_filename_is_agents_md() {
        assert_eq!(make_provider().get_instruction_filename(), "AGENTS.md");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_path_resolution_prefers_node_entrypoint_over_cmd_shim() {
        let _lock = crate::utils::wardian_test_env_lock();
        let previous_path = std::env::var_os("PATH");
        let temp = tempfile::tempdir().unwrap();
        let agy_js = temp
            .path()
            .join("node_modules")
            .join("@google")
            .join("antigravity")
            .join("bin")
            .join("agy.js");
        std::fs::create_dir_all(agy_js.parent().unwrap()).unwrap();
        std::fs::write(
            temp.path().join("agy.cmd"),
            r#"@ECHO off
SET dp0=%~dp0
"%dp0%\node.exe" "%dp0%\node_modules\@google\antigravity\bin\agy.js" %*
"#,
        )
        .unwrap();
        std::fs::write(&agy_js, "console.log('agy')").unwrap();

        unsafe {
            std::env::set_var("PATH", temp.path());
        }

        let (executable, args) = AntigravityProvider::new().get_executable();

        assert_eq!(executable, "node");
        assert_eq!(args, vec![agy_js.to_string_lossy().to_string()]);

        match previous_path {
            Some(value) => unsafe { std::env::set_var("PATH", value) },
            None => unsafe { std::env::remove_var("PATH") },
        }
    }

    #[test]
    fn spawn_args_include_context_dirs_sandbox_permissions_and_resume() {
        let provider = make_provider();
        let config = AgentConfig {
            model: Some("pro".into()),
            system_include_directories: Some(vec!["common".into(), "class".into()]),
            include_directories: Some(vec!["class".into(), "user".into()]),
            resume_session: Some("conversation-123".into()),
            ..make_antigravity_config(AntigravityProviderConfig {
                reasoning_effort: Some("high".into()),
                sandbox: Some(true),
                dangerously_skip_permissions: Some(true),
                ..Default::default()
            })
        };

        let args = provider.get_spawn_args(&config, true);

        assert_eq!(
            args,
            vec![
                "--add-dir",
                "common",
                "--add-dir",
                "class",
                "--add-dir",
                "user",
                "--sandbox",
                "--dangerously-skip-permissions",
                "--model",
                "pro",
                "--effort",
                "high",
                "--conversation",
                "conversation-123",
            ]
        );
    }

    #[test]
    fn spawn_args_project_hidden_wardian_include_roots_before_add_dir() {
        let provider = make_provider();
        let temp = tempfile::tempdir().expect("temp dir");
        let hidden = temp.path().join(".wardian").join("common");
        std::fs::create_dir_all(hidden.join(".agents").join("skills").join("role-skill"))
            .expect("create skill");
        std::fs::write(hidden.join("AGENTS.md"), "instructions").expect("write agents");
        let config = AgentConfig {
            session_id: "antigravity-session".to_string(),
            system_include_directories: Some(vec![hidden.to_string_lossy().to_string()]),
            ..make_antigravity_config(AntigravityProviderConfig::default())
        };

        let args = provider.get_spawn_args(&config, false);

        assert_eq!(args[0], "--add-dir");
        assert_ne!(args[1], hidden.to_string_lossy());
        let projected = PathBuf::from(&args[1]);
        assert!(projected.join("AGENTS.md").exists());
        assert!(projected
            .join(".agents")
            .join("skills")
            .join("role-skill")
            .exists());
    }

    #[test]
    fn parse_output_leaves_planner_responses_to_terminal_ready_detection() {
        let provider = make_provider();

        assert_eq!(
            provider
                .parse_output(r#"{"type":"USER_INPUT","source":"USER_EXPLICIT"}"#)
                .unwrap(),
            AgentEvent::UserQuery
        );
        assert_eq!(
            provider
                .parse_output(
                    r#"{"source":"MODEL","type":"PLANNER_RESPONSE","status":"DONE","content":"ok"}"#
                )
                .unwrap(),
            AgentEvent::Unknown
        );
    }

    #[test]
    fn parse_output_detects_workspace_trust_prompt_as_action_required() {
        let provider = make_provider();
        let line = "Do you trust the contents of this project? Antigravity CLI requires permission to read, edit, and execute files here.";

        assert!(matches!(
            provider.parse_output(line),
            Some(AgentEvent::ActionRequired { .. })
        ));
    }

    #[test]
    fn summarize_transcript_content_returns_last_model_response() {
        let content = concat!(
            "{\"step_index\":0,\"source\":\"USER_EXPLICIT\",\"type\":\"USER_INPUT\",\"status\":\"DONE\",\"content\":\"hello\"}\n",
            "{\"step_index\":2,\"source\":\"MODEL\",\"type\":\"PLANNER_RESPONSE\",\"status\":\"DONE\",\"content\":\"first\"}\n",
            "{\"step_index\":6,\"source\":\"MODEL\",\"type\":\"PLANNER_RESPONSE\",\"status\":\"DONE\",\"content\":\"second\"}\n",
        );

        let summary = AntigravityProvider::summarize_transcript_content(content);

        assert_eq!(summary.last_text.as_deref(), Some("second"));
        assert_eq!(summary.last_step_index, Some(6));
    }

    #[test]
    fn conversation_for_workspace_reads_cache_with_path_normalization() {
        let temp = tempfile::tempdir().expect("temp dir");
        let home = temp.path();
        std::fs::create_dir_all(home.join("cache")).expect("cache dir");
        std::fs::write(
            home.join("cache").join("last_conversations.json"),
            r#"{"C:\\Project\\Wardian":"conversation-123"}"#,
        )
        .expect("cache");

        let conversation =
            AntigravityProvider::conversation_for_workspace(home, Path::new("C:/Project/Wardian"));

        assert_eq!(conversation.as_deref(), Some("conversation-123"));
    }

    #[test]
    fn file_uri_paths_preserve_posix_and_windows_workspace_paths() {
        assert_eq!(
            file_uri_path_text("file:///tmp/wardian-workspace"),
            Some("/tmp/wardian-workspace")
        );
        assert_eq!(
            file_uri_path_text("file:///C:/Workspace/Wardian"),
            Some("C:/Workspace/Wardian")
        );
        assert_eq!(
            normalize_path_text(r"\\?\C:\Workspace\Wardian"),
            normalize_path_text("C:/Workspace/Wardian")
        );
        assert_eq!(
            normalize_path_text(r"\\?\UNC\server\share\Wardian"),
            normalize_path_text("//server/share/Wardian")
        );
    }

    fn protobuf_varint(value: u64) -> Vec<u8> {
        let mut value = value;
        let mut bytes = Vec::new();
        loop {
            let mut byte = (value & 0x7f) as u8;
            value >>= 7;
            if value != 0 {
                byte |= 0x80;
            }
            bytes.push(byte);
            if value == 0 {
                return bytes;
            }
        }
    }

    fn protobuf_string_field(field: u32, text: &str) -> Vec<u8> {
        let mut bytes = protobuf_varint(u64::from(field << 3 | 2));
        bytes.extend(protobuf_varint(text.len() as u64));
        bytes.extend(text.as_bytes());
        bytes
    }

    fn protobuf_message_field(field: u32, value: Vec<u8>) -> Vec<u8> {
        let mut bytes = protobuf_varint(u64::from(field << 3 | 2));
        bytes.extend(protobuf_varint(value.len() as u64));
        bytes.extend(value);
        bytes
    }

    #[test]
    fn verified_workspace_conversation_prefers_current_sqlite_messages() {
        let temp = tempfile::tempdir().expect("temp dir");
        let home = temp.path();
        let conversation_id = "conversation-123";
        std::fs::create_dir_all(home.join("cache")).expect("cache dir");
        std::fs::create_dir_all(home.join("conversations")).expect("conversation dir");
        std::fs::write(
            home.join("cache").join("last_conversations.json"),
            r#"{"C:\\Project\\Wardian":"conversation-123"}"#,
        )
        .expect("cache");
        std::fs::write(
            home.join("cache").join("conversation_metadata.json"),
            r#"{"conversations":{"conversation-123":{"summary":{"WorkspaceURIs":["file:///C:/Project/Wardian"]}}}}"#,
        )
        .expect("metadata");

        let database = AntigravityProvider::conversation_database_path(home, conversation_id);
        let connection = Connection::open(&database).expect("open database");
        connection
            .execute_batch(
                "CREATE TABLE steps (idx INTEGER, step_type INTEGER, step_payload BLOB);",
            )
            .expect("create steps");
        let user = protobuf_message_field(19, protobuf_string_field(2, "Make brownies."));
        let assistant = protobuf_message_field(20, protobuf_string_field(1, "Brownies made."));
        connection
            .execute(
                "INSERT INTO steps (idx, step_type, step_payload) VALUES (?1, ?2, ?3)",
                params![0_i64, 14_i64, user],
            )
            .expect("insert user");
        connection
            .execute(
                "INSERT INTO steps (idx, step_type, step_payload) VALUES (?1, ?2, ?3)",
                params![1_i64, 15_i64, assistant],
            )
            .expect("insert assistant");
        drop(connection);

        let workspace = Path::new("C:/Project/Wardian");
        assert_eq!(
            AntigravityProvider::verified_conversation_for_workspace(home, workspace, &[])
                .as_deref(),
            Some(conversation_id)
        );
        assert_eq!(
            AntigravityProvider::conversation_log_path(home, conversation_id),
            Some(database.clone())
        );
        let messages = AntigravityProvider::conversation_messages_from_database(&database)
            .expect("read conversation database");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, AgentChatRole::User);
        assert_eq!(messages[0].text, "Make brownies.");
        assert_eq!(messages[1].role, AgentChatRole::Assistant);
        assert_eq!(messages[1].text, "Brownies made.");
        assert!(AntigravityProvider::verified_conversation_for_workspace(
            home,
            workspace,
            &[conversation_id.to_string()],
        )
        .is_none());
    }
}
