use crate::args::{MemoryArgs, MemoryCommand, MemoryKindArg, MemoryScopeArg};
use crate::errors::{CliError, ExitCode};
use wardian_core::identity::{self, AgentIdentity, ListFilters, Scope};
use wardian_core::memory::{
    MemoryKind, MemorySource, MemoryStore, SaveMemoryRequest, UpdateMemoryRequest,
};

pub fn handle_memory(args: MemoryArgs) -> Result<String, CliError> {
    let store = MemoryStore::from_default_home().map_err(memory_error)?;
    let value = match args.command {
        MemoryCommand::Save {
            text,
            evidence,
            kind,
            scope,
            agent,
            workspace,
            source,
            idempotency_key,
        } => {
            let context = resolve_context(&store, agent, workspace)?;
            let workspace = match scope {
                MemoryScopeArg::Agent => None,
                MemoryScopeArg::Workspace => Some(context.workspace.ok_or_else(|| {
                    CliError::generic(
                        "workspace scope requires --workspace or a persisted workspace for the agent",
                    )
                })?),
            };
            let record = store
                .save(SaveMemoryRequest {
                    agent_id: context.agent_id,
                    workspace,
                    kind: match kind {
                        MemoryKindArg::Stable => MemoryKind::Stable,
                        MemoryKindArg::Current => MemoryKind::Current,
                    },
                    text,
                    evidence_excerpt: evidence,
                    sources: source.into_iter().map(source_from_locator).collect(),
                    idempotency_key,
                })
                .map_err(memory_error)?;
            serde_json::json!({ "schema": 1, "memory": record })
        }
        MemoryCommand::List { agent, workspace } => {
            let context = resolve_context(&store, agent, workspace)?;
            let records = store
                .list_active(&context.agent_id, context.workspace.as_deref())
                .map_err(memory_error)?;
            serde_json::json!({ "schema": 1, "memories": records })
        }
        MemoryCommand::Show { memory_id } => {
            let record = store.get(&memory_id).map_err(memory_error)?;
            authorize_record_owner(&store, &record.agent_id)?;
            serde_json::json!({ "schema": 1, "memory": record })
        }
        MemoryCommand::Update {
            memory_id,
            text,
            evidence,
            source,
            idempotency_key,
        } => {
            let existing = store.get(&memory_id).map_err(memory_error)?;
            authorize_record_owner(&store, &existing.agent_id)?;
            let record = store
                .update(UpdateMemoryRequest {
                    memory_id,
                    text,
                    evidence_excerpt: evidence,
                    sources: source.into_iter().map(source_from_locator).collect(),
                    idempotency_key,
                })
                .map_err(memory_error)?;
            serde_json::json!({ "schema": 1, "memory": record })
        }
        MemoryCommand::Remove { memory_id } => {
            let existing = store.get(&memory_id).map_err(memory_error)?;
            authorize_record_owner(&store, &existing.agent_id)?;
            let record = store.remove(&memory_id).map_err(memory_error)?;
            serde_json::json!({ "schema": 1, "memory": record })
        }
        MemoryCommand::Recall { agent, workspace } => {
            let context = resolve_context(&store, agent, workspace)?;
            let recall = store
                .recall(&context.agent_id, context.workspace.as_deref())
                .map_err(memory_error)?;
            serde_json::to_value(recall).map_err(|error| CliError::generic(error.to_string()))?
        }
        MemoryCommand::History { memory_id } => {
            let existing = store.get(&memory_id).map_err(memory_error)?;
            authorize_record_owner(&store, &existing.agent_id)?;
            let history = store.history(&memory_id).map_err(memory_error)?;
            serde_json::json!({ "schema": 1, "history": history })
        }
    };

    serde_json::to_string_pretty(&value)
        .map(|json| format!("{json}\n"))
        .map_err(|error| CliError::generic(error.to_string()))
}

struct MemoryContext {
    agent_id: String,
    workspace: Option<String>,
}

fn resolve_context(
    store: &MemoryStore,
    requested_agent: Option<String>,
    requested_workspace: Option<String>,
) -> Result<MemoryContext, CliError> {
    let caller = managed_caller(store)?;
    let target = requested_agent
        .or_else(|| caller.clone())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CliError::generic("--agent or WARDIAN_SESSION_ID is required"))?;

    let agents = agent_snapshot()?;
    let resolved = resolve_agent(&agents, &target)?;
    if let Some(caller) = caller {
        let caller = resolve_agent(&agents, &caller)?;
        if caller.uuid != resolved.uuid {
            return Err(memory_access_denied(&caller.uuid, &resolved.uuid));
        }
    }
    let agent_id = resolved.uuid.clone();
    let workspace = requested_workspace.or_else(|| resolved.workspace.clone());
    Ok(MemoryContext {
        agent_id,
        workspace,
    })
}

fn managed_caller(store: &MemoryStore) -> Result<Option<String>, CliError> {
    let Some(agent_id) = std::env::var("WARDIAN_SESSION_ID")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    let token = std::env::var(wardian_core::memory::MEMORY_CAPABILITY_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| invalid_memory_capability(&agent_id))?;
    if !store
        .validate_capability(&agent_id, &token)
        .map_err(memory_error)?
    {
        return Err(invalid_memory_capability(&agent_id));
    }
    Ok(Some(agent_id))
}

/// Full roster with the persisted database as the offline authority.
fn agent_snapshot() -> Result<Vec<AgentIdentity>, CliError> {
    if let Ok(agents) = crate::live::list_agents() {
        return Ok(agents);
    }
    let connection = crate::open_db()?;
    identity::list_agents(
        &connection,
        &ListFilters {
            scope: Scope::All,
            caller_workspace: None,
            status: None,
            class: None,
            workspace: None,
        },
    )
    .map_err(crate::identity_error)
}

fn resolve_agent<'a>(
    agents: &'a [AgentIdentity],
    target: &str,
) -> Result<&'a AgentIdentity, CliError> {
    if let Some(agent) = agents.iter().find(|agent| agent.uuid == target) {
        return Ok(agent);
    }
    let matches = agents
        .iter()
        .filter(|agent| agent.name == target)
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [] => Err(CliError::not_found(target)),
        [agent] => Ok(agent),
        _ => Err(CliError::backend(
            ExitCode::Ambiguous,
            "ambiguous_target",
            format!("Multiple agents are named {target}; pass a UUID instead"),
        )),
    }
}

fn authorize_record_owner(store: &MemoryStore, record_agent_id: &str) -> Result<(), CliError> {
    let Some(caller) = managed_caller(store)? else {
        return Ok(());
    };
    let agents = agent_snapshot()?;
    let caller = resolve_agent(&agents, &caller)?;
    if caller.uuid == record_agent_id {
        Ok(())
    } else {
        Err(memory_access_denied(&caller.uuid, record_agent_id))
    }
}

fn invalid_memory_capability(agent_id: &str) -> CliError {
    CliError::backend_with_details(
        ExitCode::Generic,
        "invalid_memory_capability",
        "managed memory commands require the capability issued to this provider process",
        serde_json::json!({ "claimed_agent_id": agent_id }),
    )
}

fn memory_access_denied(caller: &str, target: &str) -> CliError {
    CliError::backend_with_details(
        ExitCode::Generic,
        "memory_access_denied",
        "managed agents may access only their own memory",
        serde_json::json!({ "caller_agent_id": caller, "target_agent_id": target }),
    )
}

fn source_from_locator(locator: String) -> MemorySource {
    let source_type = locator
        .split_once(':')
        .map(|(source_type, _)| source_type)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("manual")
        .to_string();
    MemorySource {
        source_type,
        locator: Some(locator),
        source_hash: None,
        primary: false,
    }
}

fn memory_error(error: impl std::fmt::Display) -> CliError {
    CliError::generic(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wardian_core::db::AgentUpsert;

    struct TestHome {
        _guard: std::sync::MutexGuard<'static, ()>,
        _home: tempfile::TempDir,
        previous_home: Option<std::ffi::OsString>,
        previous_session: Option<std::ffi::OsString>,
        previous_capability: Option<std::ffi::OsString>,
    }

    impl TestHome {
        fn new() -> Self {
            let guard = crate::test_env_lock();
            let home = tempfile::tempdir().expect("temp home");
            let previous_home = std::env::var_os("WARDIAN_HOME");
            let previous_session = std::env::var_os("WARDIAN_SESSION_ID");
            let previous_capability =
                std::env::var_os(wardian_core::memory::MEMORY_CAPABILITY_ENV);
            unsafe {
                std::env::set_var("WARDIAN_HOME", home.path());
                std::env::remove_var("WARDIAN_SESSION_ID");
                std::env::remove_var(wardian_core::memory::MEMORY_CAPABILITY_ENV);
            }
            let connection = rusqlite::Connection::open(home.path().join("state.db")).unwrap();
            wardian_core::db::run_migrations(&connection).unwrap();
            for (id, name, workspace) in [
                ("agent-a", "Alpha", "C:/work/alpha"),
                ("agent-b", "Beta", "C:/work/beta"),
            ] {
                wardian_core::db::upsert_agent_with_conn(
                    &connection,
                    &AgentUpsert {
                        session_id: id,
                        session_name: name,
                        description: "",
                        agent_class: "Coder",
                        provider: "codex",
                        workspace: Some(workspace),
                        project: None,
                        is_off: true,
                        created_at: None,
                    },
                )
                .unwrap();
            }
            Self {
                _guard: guard,
                _home: home,
                previous_home,
                previous_session,
                previous_capability,
            }
        }
    }

    impl Drop for TestHome {
        fn drop(&mut self) {
            unsafe {
                match self.previous_home.take() {
                    Some(value) => std::env::set_var("WARDIAN_HOME", value),
                    None => std::env::remove_var("WARDIAN_HOME"),
                }
                match self.previous_session.take() {
                    Some(value) => std::env::set_var("WARDIAN_SESSION_ID", value),
                    None => std::env::remove_var("WARDIAN_SESSION_ID"),
                }
                match self.previous_capability.take() {
                    Some(value) => std::env::set_var(
                        wardian_core::memory::MEMORY_CAPABILITY_ENV,
                        value,
                    ),
                    None => std::env::remove_var(wardian_core::memory::MEMORY_CAPABILITY_ENV),
                }
            }
        }
    }

    fn seed(agent_id: &str, workspace: &str, text: &str) -> wardian_core::memory::MemoryRecord {
        MemoryStore::from_default_home()
            .unwrap()
            .save(SaveMemoryRequest {
                agent_id: agent_id.into(),
                workspace: Some(workspace.into()),
                kind: MemoryKind::Stable,
                text: text.into(),
                evidence_excerpt: "durable test evidence".into(),
                sources: vec![],
                idempotency_key: None,
            })
            .unwrap()
    }

    #[test]
    fn offline_name_resolution_uses_persisted_agent_uuid_and_workspace() {
        let _home = TestHome::new();
        seed("agent-a", "C:/work/alpha", "Alpha preference");

        let output = handle_memory(MemoryArgs {
            command: MemoryCommand::List {
                agent: Some("Alpha".into()),
                workspace: None,
            },
        })
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&output).unwrap();
        assert_eq!(value["memories"][0]["agent_id"], "agent-a");
        assert_eq!(value["memories"][0]["text"], "Alpha preference");
    }

    #[test]
    fn managed_agent_can_save_self_but_cannot_target_or_open_peer_memory() {
        let _home = TestHome::new();
        let peer = seed("agent-b", "C:/work/beta", "Beta preference");
        let capability = MemoryStore::from_default_home()
            .unwrap()
            .issue_capability("agent-a")
            .unwrap();
        unsafe {
            std::env::set_var("WARDIAN_SESSION_ID", "agent-a");
            std::env::set_var(wardian_core::memory::MEMORY_CAPABILITY_ENV, &capability);
        }

        let own = handle_memory(MemoryArgs {
            command: MemoryCommand::Save {
                text: "Alpha preference".into(),
                evidence: "The user chose this convention.".into(),
                kind: MemoryKindArg::Stable,
                scope: MemoryScopeArg::Workspace,
                agent: None,
                workspace: None,
                source: vec![],
                idempotency_key: None,
            },
        })
        .unwrap();
        assert!(own.contains("agent-a"));

        let target_error = handle_memory(MemoryArgs {
            command: MemoryCommand::List {
                agent: Some("Beta".into()),
                workspace: None,
            },
        })
        .unwrap_err();
        assert_eq!(target_error.code, "memory_access_denied");

        let record_error = handle_memory(MemoryArgs {
            command: MemoryCommand::Show {
                memory_id: peer.memory_id,
            },
        })
        .unwrap_err();
        assert_eq!(record_error.code, "memory_access_denied");

        unsafe { std::env::set_var("WARDIAN_SESSION_ID", "agent-b") };
        let spoofed = handle_memory(MemoryArgs {
            command: MemoryCommand::List {
                agent: None,
                workspace: None,
            },
        })
        .unwrap_err();
        assert_eq!(spoofed.code, "invalid_memory_capability");
    }
}
