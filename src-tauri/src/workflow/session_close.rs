//! Generic conversation-boundary workflow invoker.

use crate::workflow::runs;
use fs2::FileExt;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};
use wardian_core::models::InvocationKind;

#[derive(Debug, Clone)]
pub struct SessionCloseContext {
    /// Unique lifecycle boundary identity. Retries of one launched workflow
    /// retain this value; later archive-less boundaries never collide.
    pub boundary_id: String,
    pub agent_id: String,
    pub agent_name: String,
    pub workspace: String,
    pub provider: String,
    pub boundary_reason: String,
    pub archive_available: bool,
    pub conversation_id: Option<String>,
    pub source_sequence: Option<u64>,
}

impl SessionCloseContext {
    pub fn durable_intent(&self) -> wardian_core::agent_replacement::SessionCloseIntent {
        wardian_core::agent_replacement::SessionCloseIntent {
            boundary_id: self.boundary_id.clone(),
            agent_id: self.agent_id.clone(),
            agent_name: self.agent_name.clone(),
            workspace: self.workspace.clone(),
            provider: self.provider.clone(),
            boundary_reason: self.boundary_reason.clone(),
            archive_available: self.archive_available,
            conversation_id: self.conversation_id.clone(),
            source_sequence: self.source_sequence,
        }
    }
}

impl From<wardian_core::agent_replacement::SessionCloseIntent> for SessionCloseContext {
    fn from(intent: wardian_core::agent_replacement::SessionCloseIntent) -> Self {
        Self {
            boundary_id: intent.boundary_id,
            agent_id: intent.agent_id,
            agent_name: intent.agent_name,
            workspace: intent.workspace,
            provider: intent.provider,
            boundary_reason: intent.boundary_reason,
            archive_available: intent.archive_available,
            conversation_id: intent.conversation_id,
            source_sequence: intent.source_sequence,
        }
    }
}

pub async fn invoke_matching(app: AppHandle, context: SessionCloseContext) -> Result<(), String> {
    let invokers =
        wardian_core::session_close::matching_invokers(&context.agent_id, &context.boundary_reason);
    for invoker in invokers {
        if !wardian_core::session_close::archive_requirement_satisfied(
            &invoker,
            context.archive_available,
        ) {
            crate::utils::logging::log_debug(&format!(
                "[workflow] session-close invoker {} skipped because no archive is available",
                invoker.id
            ));
            continue;
        }
        launch(app.clone(), invoker, context.clone()).await?;
    }
    Ok(())
}

async fn launch(
    app: AppHandle,
    invoker: wardian_core::session_close::WorkflowSessionCloseInvoker,
    context: SessionCloseContext,
) -> Result<(), String> {
    let path = wardian_core::workflow::resolve_blueprint_path(&invoker.blueprint_id)
        .ok_or_else(|| format!("could not resolve blueprint {}", invoker.blueprint_id))?;
    let blueprint = wardian_core::workflow::parse_file(&path).map_err(|error| error.to_string())?;
    let report = wardian_core::workflow::validate(&blueprint);
    if !report.is_valid() {
        return Err(format!("blueprint {} is invalid", blueprint.id));
    }
    let run_id = session_close_run_id(&invoker.id, &context.boundary_id);
    let run_root = wardian_core::paths::workflow_run_dir(&blueprint.id, &run_id)
        .ok_or_else(|| "could not resolve workflow run directory".to_string())?;
    let provider = invoker.provider.unwrap_or_else(|| {
        crate::utils::load_shell_settings()
            .map(|settings| settings.default_provider)
            .unwrap_or_else(|_| "codex".to_string())
    });
    let workspace = invoker
        .workspace
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(&context.workspace));
    let mut input = match invoker.input {
        Value::Object(map) => map,
        _ => serde_json::Map::new(),
    };
    input.insert("agent_id".into(), Value::String(context.agent_id.clone()));
    input.insert("agent_name".into(), Value::String(context.agent_name));
    input.insert("workspace".into(), Value::String(context.workspace));
    input.insert("source_provider".into(), Value::String(context.provider));
    input.insert(
        "boundary_reason".into(),
        Value::String(context.boundary_reason),
    );
    input.insert(
        "boundary_id".into(),
        Value::String(context.boundary_id.clone()),
    );
    input.insert(
        "archive_available".into(),
        Value::Bool(context.archive_available),
    );
    input.insert(
        "conversation_id".into(),
        context
            .conversation_id
            .clone()
            .map(Value::String)
            .unwrap_or_else(|| Value::String(String::new())),
    );
    input.insert(
        "source_sequence".into(),
        Value::from(context.source_sequence.unwrap_or(0)),
    );
    input.insert(
        "idempotency_key".into(),
        Value::String(session_close_idempotency_key(
            &invoker.id,
            &context.boundary_id,
        )),
    );
    let assignments = wardian_core::workflow::assignment::normalize_assignments(
        Some(invoker.assignments),
        &invoker.bindings,
        InvocationKind::Scheduled,
    );
    let state = app.state::<crate::state::AppState>();
    let catalog = runs::agent_catalog_from_state_with_assignments(
        &state,
        &invoker.bindings,
        &assignments,
        &workspace,
        &provider,
    )
    .await;
    let memory_principal = context.agent_id.clone();
    let Some(claim) = claim_session_close_run(&run_root, &run_id)? else {
        return Ok(());
    };
    let run_state = runs::prepare_new_run_with_assignments_and_memory_principal(
        &blueprint,
        &run_id,
        &run_root,
        &workspace,
        &provider,
        &invoker.bindings,
        &assignments,
        Value::Object(input),
        Some(memory_principal.clone()),
    )?;
    drop(claim);
    let blueprint_for_inbox = blueprint.clone();
    let run_root_for_inbox = run_root.clone();
    let app_for_inbox = app.clone();
    tauri::async_runtime::spawn(async move {
        let result = runs::drive_started_run_with_catalog_assignments_and_memory_principal(
            Some(app),
            blueprint,
            run_state,
            run_root,
            workspace,
            provider,
            invoker.bindings,
            assignments,
            catalog,
            Some(memory_principal),
        )
        .await;
        if let Err(error) = result {
            crate::utils::logging::log_debug(&format!(
                "[workflow] session-close run failed: {error}"
            ));
        }
        runs::emit_workflow_inbox_update(&app_for_inbox, &blueprint_for_inbox, &run_root_for_inbox);
    });
    Ok(())
}

fn session_close_idempotency_key(invoker_id: &str, boundary_id: &str) -> String {
    format!("session-close:{invoker_id}:{boundary_id}")
}

fn session_close_run_id(invoker_id: &str, boundary_id: &str) -> String {
    let digest = Sha256::digest(format!("{invoker_id}\0{boundary_id}").as_bytes());
    format!("session-close-{:x}", digest)
}

fn claim_session_close_run(
    run_root: &std::path::Path,
    run_id: &str,
) -> Result<Option<std::fs::File>, String> {
    let parent = run_root
        .parent()
        .ok_or_else(|| "session-close run has no parent directory".to_string())?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("could not create session-close run directory: {error}"))?;
    let claim_path = parent.join(format!(".{run_id}.claim.lock"));
    let claim = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&claim_path)
        .map_err(|error| format!("could not open session-close run claim: {error}"))?;
    FileExt::lock_exclusive(&claim)
        .map_err(|error| format!("could not lock session-close run claim: {error}"))?;
    if run_root.join("state.json").is_file() {
        return Ok(None);
    }
    if run_root.exists() {
        if run_root.file_name() != Some(std::ffi::OsStr::new(run_id)) {
            return Err("refusing to repair an unexpected session-close run path".to_string());
        }
        std::fs::remove_dir_all(run_root)
            .map_err(|error| format!("could not repair partial session-close run: {error}"))?;
    }
    std::fs::create_dir(run_root)
        .map_err(|error| format!("could not create session-close run: {error}"))?;
    Ok(Some(claim))
}

#[cfg(test)]
mod tests {
    use super::{claim_session_close_run, session_close_idempotency_key, session_close_run_id};
    use wardian_core::memory::{
        MemoryActor, MemoryCommitBatch, MemoryKind, MemoryMutation, MemoryStore,
    };

    #[test]
    fn archive_less_boundaries_receive_distinct_idempotency_keys() {
        let first = session_close_idempotency_key("invoker", "boundary-a");
        let retry = session_close_idempotency_key("invoker", "boundary-a");
        let second = session_close_idempotency_key("invoker", "boundary-b");
        assert_eq!(first, retry);
        assert_ne!(first, second);
    }

    #[test]
    fn one_boundary_and_invoker_have_one_deterministic_run_id() {
        assert_eq!(
            session_close_run_id("invoker", "boundary-a"),
            session_close_run_id("invoker", "boundary-a")
        );
        assert_ne!(
            session_close_run_id("invoker", "boundary-a"),
            session_close_run_id("invoker", "boundary-b")
        );
    }

    #[test]
    fn partial_deterministic_run_is_repaired_before_it_is_claimed() {
        let temp = tempfile::tempdir().expect("temporary workflow root");
        let run_id = session_close_run_id("invoker", "boundary-a");
        let run_root = temp.path().join(&run_id);
        std::fs::create_dir(&run_root).expect("partial run directory");
        std::fs::write(run_root.join("partial.txt"), "incomplete").expect("partial marker");

        let claim = claim_session_close_run(&run_root, &run_id)
            .expect("repair partial run")
            .expect("run should be claimable");
        assert!(run_root.is_dir());
        assert!(!run_root.join("partial.txt").exists());
        std::fs::write(run_root.join("state.json"), "{}").expect("durable run state");
        drop(claim);

        assert!(claim_session_close_run(&run_root, &run_id)
            .expect("recognize accepted run")
            .is_none());
    }

    #[test]
    fn archive_less_boundaries_commit_independently() {
        let temp = tempfile::tempdir().expect("temp memory store");
        let store = MemoryStore::open(temp.path().join("memory.db")).expect("memory store");
        let actor = MemoryActor::agent("agent-a");
        for (boundary_id, text) in [
            ("boundary-a", "First archive-less boundary"),
            ("boundary-b", "Second archive-less boundary"),
        ] {
            store
                .commit_batch(
                    &actor,
                    MemoryCommitBatch {
                        agent_id: "agent-a".into(),
                        workspace: Some("workspace".into()),
                        idempotency_key: session_close_idempotency_key("invoker", boundary_id),
                        operations: vec![MemoryMutation::Save {
                            kind: MemoryKind::Current,
                            text: text.into(),
                            evidence_excerpt: format!("Evidence from {boundary_id}"),
                            sources: vec![],
                        }],
                        cursor: None,
                    },
                )
                .expect("independent boundary commit");
        }

        let memories = store
            .list_active(&actor, "agent-a", Some("workspace"))
            .expect("list committed boundaries");
        assert_eq!(memories.len(), 2);
    }
}
