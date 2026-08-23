use crate::args::{MemoryArgs, MemoryCommand, MemoryKindArg, MemoryScopeArg};
use crate::errors::CliError;
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
            let context = resolve_context(agent, workspace)?;
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
            let context = resolve_context(agent, workspace)?;
            let records = store
                .list_active(&context.agent_id, context.workspace.as_deref())
                .map_err(memory_error)?;
            serde_json::json!({ "schema": 1, "memories": records })
        }
        MemoryCommand::Show { memory_id } => {
            let record = store.get(&memory_id).map_err(memory_error)?;
            serde_json::json!({ "schema": 1, "memory": record })
        }
        MemoryCommand::Update {
            memory_id,
            text,
            evidence,
            source,
            idempotency_key,
        } => {
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
            let record = store.remove(&memory_id).map_err(memory_error)?;
            serde_json::json!({ "schema": 1, "memory": record })
        }
        MemoryCommand::Recall { agent, workspace } => {
            let context = resolve_context(agent, workspace)?;
            let recall = store
                .recall(&context.agent_id, context.workspace.as_deref())
                .map_err(memory_error)?;
            serde_json::to_value(recall).map_err(|error| CliError::generic(error.to_string()))?
        }
        MemoryCommand::History { memory_id } => {
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
    requested_agent: Option<String>,
    requested_workspace: Option<String>,
) -> Result<MemoryContext, CliError> {
    let target = requested_agent
        .or_else(|| std::env::var("WARDIAN_SESSION_ID").ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CliError::generic("--agent or WARDIAN_SESSION_ID is required"))?;

    let resolved = crate::live::list_agents().ok().and_then(|agents| {
        agents
            .into_iter()
            .find(|agent| agent.uuid == target || agent.name == target)
    });
    let agent_id = resolved
        .as_ref()
        .map(|agent| agent.uuid.clone())
        .unwrap_or(target);
    let workspace = requested_workspace.or_else(|| resolved.and_then(|agent| agent.workspace));
    Ok(MemoryContext {
        agent_id,
        workspace,
    })
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
