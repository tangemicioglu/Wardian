use crate::engine::core::{self, finalize_if_done, is_approval, step};
use crate::engine::event::{Event, EventKind};
use crate::engine::executor::*;
use crate::engine::graph::Graph;
use crate::engine::interpolate::resolve;
use crate::engine::state::{NodeStatus, RunState, RunStatus};
use crate::engine::store::{append_event, read_checkpoint, read_events, write_checkpoint};
use crate::engine::{EngineError, StepError};
use crate::workflow::{Blueprint, Node};
use std::path::Path;

/// The async engine: drives a run by repeatedly consulting the pure core,
/// executing side-effecting nodes through a `StepExecutor`, and persisting each
/// event + checkpoint under `run_root`.
pub struct Engine;

impl Engine {
    /// Start a fresh run and drive it until it completes, fails, or parks on an
    /// approval. Returns the resulting `RunState`.
    pub async fn start(
        bp: &Blueprint,
        trigger: serde_json::Value,
        run_root: &Path,
        exec: &dyn StepExecutor,
    ) -> crate::engine::Result<RunState> {
        Self::start_with_id(bp, new_run_id(), trigger, run_root, exec).await
    }

    /// Start a fresh run with a caller-supplied run id and drive it until it
    /// completes, fails, or parks on an approval. Returns the resulting
    /// `RunState`.
    pub async fn start_with_id(
        bp: &Blueprint,
        run_id: impl Into<String>,
        trigger: serde_json::Value,
        run_root: &Path,
        exec: &dyn StepExecutor,
    ) -> crate::engine::Result<RunState> {
        let s = Self::initialize_with_id(bp, run_id, trigger, run_root)?;
        Self::drive_from_state(bp, s, run_root, exec).await
    }

    /// Initialize a fresh run by writing the invocation-independent
    /// `RunStarted` event and checkpoint. Callers that detach long-running
    /// execution can use this as the durable startup acknowledgement.
    pub fn initialize_with_id(
        bp: &Blueprint,
        run_id: impl Into<String>,
        trigger: serde_json::Value,
        run_root: &Path,
    ) -> crate::engine::Result<RunState> {
        let g = Graph::new(bp);
        let mut s = RunState::new(run_id.into(), &bp.id);
        emit(
            run_root,
            &g,
            &mut s,
            EventKind::RunStarted {
                blueprint_id: bp.id.clone(),
                schema: bp.schema,
                trigger,
            },
        )?;
        Ok(s)
    }

    /// Continue driving an already-initialized run state.
    pub async fn drive_from_state(
        bp: &Blueprint,
        mut s: RunState,
        run_root: &Path,
        exec: &dyn StepExecutor,
    ) -> crate::engine::Result<RunState> {
        let g = Graph::new(bp);
        drive(&g, &mut s, run_root, exec).await?;
        Ok(s)
    }

    /// Resume a parked/crashed run from its on-disk state and keep driving.
    pub async fn resume(
        bp: &Blueprint,
        run_root: &Path,
        exec: &dyn StepExecutor,
    ) -> crate::engine::Result<RunState> {
        let g = Graph::new(bp);
        let mut s = load_state(&g, run_root)?;
        if s.status == RunStatus::AwaitingApproval {
            return Ok(s); // still needs a human; grant_approval drives it onward
        }
        // Re-mark any mid-flight Running (non-loop) nodes back to Pending so they re-dispatch.
        let running: Vec<String> = s
            .nodes
            .iter()
            .filter(|(_, st)| **st == NodeStatus::Running)
            .map(|(id, _)| id.clone())
            .collect();
        for id in running {
            if g.blueprint()
                .find_node(&id)
                .map(|n| n.r#type != "loop")
                .unwrap_or(true)
            {
                s.set_node_status(&id, NodeStatus::Pending);
            }
        }
        drive(&g, &mut s, run_root, exec).await?;
        Ok(s)
    }

    /// Reconstruct `RunState` purely by replaying the event log (no execution).
    pub fn replay(bp: &Blueprint, run_root: &Path) -> crate::engine::Result<RunState> {
        let g = Graph::new(bp);
        let mut s = RunState::new("replay", &bp.id);
        for ev in read_events(run_root)? {
            core::apply(&g, &mut s, &ev)?;
        }
        Ok(s)
    }

    /// Grant approval on a parked run, then continue driving.
    pub async fn grant_approval(
        bp: &Blueprint,
        run_root: &Path,
        node: &str,
        actor: &str,
        note: Option<String>,
        exec: &dyn StepExecutor,
    ) -> crate::engine::Result<RunState> {
        let s = Self::record_approval_granted(bp, run_root, node, actor, note)?;
        Self::drive_from_state(bp, s, run_root, exec).await
    }

    /// Persist an approval decision without executing the remainder of the
    /// workflow. Callers that detach long-running continuation work can use
    /// the returned running state with [`Self::drive_from_state`].
    pub fn record_approval_granted(
        bp: &Blueprint,
        run_root: &Path,
        node: &str,
        actor: &str,
        note: Option<String>,
    ) -> crate::engine::Result<RunState> {
        let _approval_decision = acquire_approval_decision_guard(run_root)?;
        let g = Graph::new(bp);
        let mut s = load_state(&g, run_root)?;
        if s.status != RunStatus::AwaitingApproval {
            return Err(EngineError::NotAwaitingApproval(node.into()));
        }
        emit(
            run_root,
            &g,
            &mut s,
            EventKind::ApprovalGranted {
                node: node.into(),
                actor: actor.into(),
                note,
            },
        )?;
        Ok(s)
    }

    /// Reject approval on a parked run (fails the run).
    pub async fn reject_approval(
        bp: &Blueprint,
        run_root: &Path,
        node: &str,
        actor: &str,
        note: Option<String>,
    ) -> crate::engine::Result<RunState> {
        let _approval_decision = acquire_approval_decision_guard(run_root)?;
        let g = Graph::new(bp);
        let mut s = load_state(&g, run_root)?;
        if s.status != RunStatus::AwaitingApproval {
            return Err(EngineError::NotAwaitingApproval(node.into()));
        }
        emit(
            run_root,
            &g,
            &mut s,
            EventKind::ApprovalRejected {
                node: node.into(),
                actor: actor.into(),
                note,
            },
        )?;
        Ok(s)
    }
}

fn acquire_approval_decision_guard(
    run_root: &Path,
) -> crate::engine::Result<crate::workflow_approval_lock::ApprovalDecisionGuard> {
    match crate::workflow_approval_lock::acquire_approval_decision_guard(run_root) {
        Ok(guard) => Ok(guard),
        Err(crate::workflow_approval_lock::ApprovalDecisionLockError::Contended) => {
            Err(EngineError::ApprovalDecisionInProgress)
        }
        Err(crate::workflow_approval_lock::ApprovalDecisionLockError::Io(error)) => {
            Err(EngineError::Io(error))
        }
    }
}

pub fn new_run_id() -> String {
    format!(
        "{}-{}",
        chrono::Utc::now().timestamp_millis(),
        &uuid::Uuid::new_v4().to_string()[..8]
    )
}

fn load_state(g: &Graph<'_>, run_root: &Path) -> crate::engine::Result<RunState> {
    if let Some(s) = read_checkpoint(run_root)? {
        return Ok(s);
    }
    // No checkpoint: rebuild from the log.
    let mut s = RunState::new("rebuilt", &g.blueprint().id);
    for ev in read_events(run_root)? {
        core::apply(g, &mut s, &ev)?;
    }
    Ok(s)
}

/// Emit: stamp seq, fold via `apply`, append to log, checkpoint.
fn emit(
    run_root: &Path,
    g: &Graph<'_>,
    s: &mut RunState,
    kind: EventKind,
) -> crate::engine::Result<()> {
    let ev = Event::new(s.next_seq, kind);
    core::apply(g, s, &ev)?;
    append_event(run_root, &ev)?;
    write_checkpoint(run_root, s)?;
    Ok(())
}

/// The main loop: advance loops, finalize, then dispatch each runnable node.
async fn drive(
    g: &Graph<'_>,
    s: &mut RunState,
    run_root: &Path,
    exec: &dyn StepExecutor,
) -> crate::engine::Result<()> {
    loop {
        core::advance_loops(g, s);
        finalize_if_done(g, s);
        write_checkpoint(run_root, s)?;
        if s.status != RunStatus::Running {
            if s.status == RunStatus::Completed {
                emit(run_root, g, s, EventKind::RunCompleted)?;
            }
            return Ok(());
        }
        let runnable = step(g, s);
        if runnable.is_empty() {
            // No progress possible but not finalized: guard against a stuck graph.
            return Ok(());
        }
        for node_id in runnable {
            dispatch(g, s, run_root, exec, &node_id).await?;
            if s.status == RunStatus::AwaitingApproval || s.status == RunStatus::Failed {
                return Ok(());
            }
        }
    }
}

/// Execute one runnable node: control nodes in-engine, side-effecting via the executor.
async fn dispatch(
    g: &Graph<'_>,
    s: &mut RunState,
    run_root: &Path,
    exec: &dyn StepExecutor,
    node_id: &str,
) -> crate::engine::Result<()> {
    let node = g
        .blueprint()
        .find_node(node_id)
        .ok_or_else(|| EngineError::InvalidState(format!("missing node {node_id}")))?
        .clone();

    // Triggers + join: pass-through completion.
    if node.r#type.ends_with("_trigger") || node.r#type == "manual_trigger" || node.r#type == "join"
    {
        let output = if node.r#type.ends_with("_trigger") || node.r#type == "manual_trigger" {
            s.registry
                .get("trigger")
                .and_then(|value| value.get("output"))
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}))
        } else {
            serde_json::json!({})
        };
        emit(
            run_root,
            g,
            s,
            EventKind::NodeCompleted {
                node: node.id.clone(),
                output,
            },
        )?;
        return Ok(());
    }
    if is_approval(g, node_id) {
        emit(
            run_root,
            g,
            s,
            EventKind::AwaitingApproval {
                node: node.id.clone(),
            },
        )?;
        return Ok(());
    }
    if node.r#type == "loop" {
        emit_loop_enter(run_root, g, s, node_id)?;
        return Ok(());
    }
    if node.r#type == "branch" {
        let port = eval_branch(s, &node)?;
        emit(
            run_root,
            g,
            s,
            EventKind::BranchTaken {
                node: node.id.clone(),
                port,
            },
        )?;
        return Ok(());
    }

    emit(
        run_root,
        g,
        s,
        EventKind::NodeStarted {
            node: node.id.clone(),
        },
    )?;
    let result = run_side_effect(g, s, exec, &node).await;
    match result {
        Ok(out) => emit(
            run_root,
            g,
            s,
            EventKind::NodeCompleted {
                node: node.id.clone(),
                output: out,
            },
        )?,
        Err(error) => {
            if error.skipped_reason().is_some() {
                emit(
                    run_root,
                    g,
                    s,
                    EventKind::NodeSkipped {
                        node: node.id.clone(),
                    },
                )?
            } else {
                emit(
                    run_root,
                    g,
                    s,
                    EventKind::NodeFailed {
                        node: node.id.clone(),
                        error: error.0,
                    },
                )?
            }
        }
    }
    Ok(())
}

/// Loop entry must record the iteration event AND run the core `enter_loop` side
/// effects (status/ports). We emit `LoopIteration{0}` (folded by apply) then
/// pulse the body via the core helper, persisting the resulting state.
fn emit_loop_enter(
    run_root: &Path,
    g: &Graph<'_>,
    s: &mut RunState,
    loop_id: &str,
) -> crate::engine::Result<()> {
    let ev = Event::new(
        s.next_seq,
        EventKind::LoopIteration {
            node: loop_id.into(),
            iteration: 0,
        },
    );
    core::apply(g, s, &ev)?;
    append_event(run_root, &ev)?;
    core::enter_loop(g, s, loop_id);
    write_checkpoint(run_root, s)?;
    Ok(())
}

fn eval_branch(s: &RunState, node: &Node) -> crate::engine::Result<String> {
    // Minimal condition: truthiness of a registry path named in `condition`.
    let cond = node
        .fields
        .get("condition")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let truthy = core::lookup_truthy(&s.registry, cond);
    Ok(if truthy {
        "on_true".into()
    } else {
        "on_false".into()
    })
}

/// Interpolate the node's string fields and call the matching executor method.
async fn run_side_effect(
    g: &Graph<'_>,
    s: &RunState,
    exec: &dyn StepExecutor,
    node: &Node,
) -> Result<serde_json::Value, StepError> {
    let _ = g;
    let f = |key: &str| -> Result<String, StepError> {
        let raw = node.fields.get(key).and_then(|v| v.as_str()).unwrap_or("");
        resolve(raw, &s.registry).map_err(|p| StepError::new(format!("unresolved {{{{{p}}}}}")))
    };
    match node.r#type.as_str() {
        "task" => Ok(exec
            .run_agent_task(AgentTaskRequest {
                node: node.id.clone(),
                agent: f("agent")?,
                prompt: f("prompt")?,
                output_schema: node
                    .fields
                    .get("output_schema")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
            })
            .await?
            .0),
        "decision" => {
            let choices = node
                .fields
                .get("choices")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|c| c.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let port = exec
                .run_decision(DecisionRequest {
                    node: node.id.clone(),
                    agent: f("agent")?,
                    prompt: f("prompt")?,
                    choices,
                })
                .await?
                .0;
            // Encode the chosen port as output; routing handled by DecisionMade in a follow-up.
            Ok(serde_json::json!({ "chosen": port }))
        }
        "shell" => Ok(exec
            .run_shell(ShellRequest {
                node: node.id.clone(),
                command: f("command")?,
                cwd: node
                    .fields
                    .get("cwd")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
            })
            .await?
            .0),
        "script" => Ok(exec
            .run_script(ScriptRequest {
                node: node.id.clone(),
                runtime: f("runtime")?,
                path: f("path")?,
            })
            .await?
            .0),
        "notify" => {
            exec.notify(NotifyRequest {
                node: node.id.clone(),
                message: f("message")?,
            })
            .await?;
            Ok(serde_json::json!({}))
        }
        "state" => Ok(exec
            .state_op(StateRequest {
                node: node.id.clone(),
                op: f("op")?,
                entries: node
                    .fields
                    .get("entries")
                    .cloned()
                    .unwrap_or(serde_json::json!({})),
            })
            .await?
            .0),
        "memory_commit" => {
            let source_node = f("source_node")?;
            let principal_template = node
                .fields
                .get("agent_id")
                .and_then(|value| value.as_str())
                .map(str::trim)
                .unwrap_or_default();
            if principal_template != "{{trigger.output.agent_id}}" {
                return Err(StepError::new(
                    "memory_commit agent_id must be the invocation-owned {{trigger.output.agent_id}}",
                ));
            }
            let agent_id = f("agent_id")?;
            let payload = s.node_output(&source_node).cloned().ok_or_else(|| {
                StepError::new(format!(
                    "memory_commit source node `{source_node}` has no output"
                ))
            })?;
            let trigger_output = s.registry.get("trigger").and_then(|value| value.get("output"));
            let trigger_string = |field: &str| {
                trigger_output
                    .and_then(|value| value.get(field))
                    .and_then(|value| value.as_str())
                    .map(str::to_string)
            };
            Ok(exec
                .memory_commit(MemoryCommitRequest {
                    node: node.id.clone(),
                    agent_id,
                    workspace: trigger_string("workspace"),
                    conversation_id: trigger_string("conversation_id"),
                    source_sequence: trigger_output
                        .and_then(|value| value.get("source_sequence"))
                        .and_then(|value| value.as_u64()),
                    archive_available: trigger_output
                        .and_then(|value| value.get("archive_available"))
                        .and_then(|value| value.as_bool()),
                    idempotency_key: trigger_string("idempotency_key"),
                    payload,
                })
                .await?
                .0)
        }
        other => Err(StepError::new(format!(
            "no executor for node type `{other}`"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::MockExecutor;

    #[tokio::test]
    async fn start_with_id_persists_caller_supplied_run_id() {
        let dir = tempfile::tempdir().unwrap();
        let blueprint = Blueprint {
            schema: 2,
            id: "wf".into(),
            name: "Workflow".into(),
            nodes: vec![Node {
                id: "t".into(),
                r#type: "manual_trigger".into(),
                name: None,
                parent: None,
                fields: serde_json::Map::new(),
                position: None,
            }],
            edges: vec![],
            body: String::new(),
        };
        let exec = MockExecutor::new();

        let state = Engine::start_with_id(
            &blueprint,
            "run-xyz",
            serde_json::json!({}),
            dir.path(),
            &exec,
        )
        .await
        .unwrap();

        assert_eq!(state.run_id, "run-xyz");
        let checkpoint = read_checkpoint(dir.path()).unwrap().unwrap();
        assert_eq!(checkpoint.run_id, "run-xyz");
    }

    fn approval_blueprint() -> Blueprint {
        Blueprint {
            schema: 2,
            id: "wf".into(),
            name: "Workflow".into(),
            nodes: vec![
                Node {
                    id: "trigger".into(),
                    r#type: "manual_trigger".into(),
                    name: None,
                    parent: None,
                    fields: serde_json::Map::new(),
                    position: None,
                },
                Node {
                    id: "gate".into(),
                    r#type: "approval".into(),
                    name: None,
                    parent: None,
                    fields: serde_json::Map::new(),
                    position: None,
                },
                Node {
                    id: "task".into(),
                    r#type: "task".into(),
                    name: None,
                    parent: None,
                    fields: serde_json::json!({
                        "agent": "role:worker",
                        "prompt": "work"
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                    position: None,
                },
            ],
            edges: vec![
                crate::workflow::Edge {
                    from: "trigger".into(),
                    from_port: "out".into(),
                    to: "gate".into(),
                    to_port: "in".into(),
                },
                crate::workflow::Edge {
                    from: "gate".into(),
                    from_port: "out".into(),
                    to: "task".into(),
                    to_port: "in".into(),
                },
            ],
            body: String::new(),
        }
    }

    #[tokio::test]
    async fn record_approval_granted_persists_running_state_before_continuation() {
        let dir = tempfile::tempdir().unwrap();
        let blueprint = approval_blueprint();
        let exec = MockExecutor::new();

        let parked = Engine::start_with_id(
            &blueprint,
            "run-xyz",
            serde_json::json!({}),
            dir.path(),
            &exec,
        )
        .await
        .unwrap();
        assert_eq!(parked.status, RunStatus::AwaitingApproval);

        let accepted =
            Engine::record_approval_granted(&blueprint, dir.path(), "gate", "user", None).unwrap();

        assert_eq!(accepted.status, RunStatus::Running);
        assert_eq!(
            read_checkpoint(dir.path()).unwrap().unwrap().status,
            RunStatus::Running
        );
        assert!(!exec.calls().contains(&"task:work".to_string()));
    }

    #[tokio::test]
    async fn approval_transitions_reject_another_in_progress_decision() {
        let dir = tempfile::tempdir().unwrap();
        let blueprint = approval_blueprint();
        let exec = MockExecutor::new();
        Engine::start_with_id(
            &blueprint,
            "run-xyz",
            serde_json::json!({}),
            dir.path(),
            &exec,
        )
        .await
        .unwrap();
        let _approval_decision =
            crate::workflow_approval_lock::acquire_approval_decision_guard(dir.path()).unwrap();

        assert!(matches!(
            Engine::record_approval_granted(&blueprint, dir.path(), "gate", "user", None),
            Err(EngineError::ApprovalDecisionInProgress)
        ));
        assert!(matches!(
            Engine::reject_approval(&blueprint, dir.path(), "gate", "user", None).await,
            Err(EngineError::ApprovalDecisionInProgress)
        ));
    }

    #[test]
    fn initialize_with_id_persists_started_checkpoint_before_driving() {
        let dir = tempfile::tempdir().unwrap();
        let blueprint = Blueprint {
            schema: 2,
            id: "wf".into(),
            name: "Workflow".into(),
            nodes: vec![Node {
                id: "t".into(),
                r#type: "manual_trigger".into(),
                name: None,
                parent: None,
                fields: serde_json::Map::new(),
                position: None,
            }],
            edges: vec![],
            body: String::new(),
        };

        let state = Engine::initialize_with_id(
            &blueprint,
            "run-xyz",
            serde_json::json!({"source":"manual"}),
            dir.path(),
        )
        .unwrap();

        assert_eq!(state.run_id, "run-xyz");
        let checkpoint = read_checkpoint(dir.path()).unwrap().unwrap();
        assert_eq!(checkpoint.run_id, "run-xyz");
        assert_eq!(checkpoint.next_seq, 1);
        let events = read_events(dir.path()).unwrap();
        assert!(matches!(
            events.first().map(|event| &event.kind),
            Some(EventKind::RunStarted { .. })
        ));
    }

    #[tokio::test]
    async fn trigger_node_outputs_runtime_timestamp() {
        let dir = tempfile::tempdir().unwrap();
        let blueprint = Blueprint {
            schema: 2,
            id: "wf".into(),
            name: "Workflow".into(),
            nodes: vec![Node {
                id: "trigger".into(),
                r#type: "manual_trigger".into(),
                name: None,
                parent: None,
                fields: serde_json::Map::new(),
                position: None,
            }],
            edges: vec![],
            body: String::new(),
        };
        let exec = MockExecutor::new();

        let state = Engine::start_with_id(
            &blueprint,
            "run-xyz",
            serde_json::json!({"source":"manual"}),
            dir.path(),
            &exec,
        )
        .await
        .unwrap();

        let global_timestamp = state.registry["trigger"]["output"]["timestamp"]
            .as_str()
            .expect("global trigger timestamp");
        let node_timestamp = state.registry["nodes"]["trigger"]["output"]["timestamp"]
            .as_str()
            .expect("trigger node timestamp");

        assert_eq!(node_timestamp, global_timestamp);
        assert_eq!(
            state.registry["nodes"]["trigger"]["output"]["source"],
            "manual"
        );
    }

    #[tokio::test]
    async fn skipped_step_emits_node_skipped_instead_of_failing_run() {
        let dir = tempfile::tempdir().unwrap();
        let blueprint = Blueprint {
            schema: 2,
            id: "wf".into(),
            name: "Workflow".into(),
            nodes: vec![
                Node {
                    id: "start".into(),
                    r#type: "manual_trigger".into(),
                    name: None,
                    parent: None,
                    fields: serde_json::Map::new(),
                    position: None,
                },
                Node {
                    id: "task".into(),
                    r#type: "task".into(),
                    name: None,
                    parent: None,
                    fields: serde_json::json!({
                        "agent": "role:worker",
                        "prompt": "work"
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                    position: None,
                },
            ],
            edges: vec![crate::workflow::Edge {
                from: "start".into(),
                from_port: "out".into(),
                to: "task".into(),
                to_port: "in".into(),
            }],
            body: String::new(),
        };
        let exec = MockExecutor::new().with_skipped("task", "busy");

        let state = Engine::start_with_id(
            &blueprint,
            "run-xyz",
            serde_json::json!({}),
            dir.path(),
            &exec,
        )
        .await
        .unwrap();
        let events = read_events(dir.path()).unwrap();

        assert_eq!(state.status, RunStatus::Completed);
        assert_eq!(state.nodes["task"], NodeStatus::Skipped);
        assert!(events.iter().any(|event| matches!(
            event.kind,
            EventKind::NodeSkipped { ref node } if node == "task"
        )));
        assert!(!events
            .iter()
            .any(|event| matches!(event.kind, EventKind::NodeFailed { .. })));
    }

    #[tokio::test]
    async fn memory_commit_receives_the_named_upstream_output() {
        let dir = tempfile::tempdir().unwrap();
        let payload = serde_json::json!({
            "agent_id": "agent-a",
            "idempotency_key": "run-1:conv-1:4",
            "operations": []
        });
        let blueprint = Blueprint {
            schema: 2,
            id: "memory-workflow".into(),
            name: "Memory workflow".into(),
            nodes: vec![
                Node {
                    id: "trigger".into(),
                    r#type: "manual_trigger".into(),
                    name: None,
                    parent: None,
                    fields: serde_json::Map::new(),
                    position: None,
                },
                Node {
                    id: "extract".into(),
                    r#type: "task".into(),
                    name: None,
                    parent: None,
                    fields: serde_json::json!({"agent":"role:curator","prompt":"extract"})
                        .as_object()
                        .unwrap()
                        .clone(),
                    position: None,
                },
                Node {
                    id: "commit".into(),
                    r#type: "memory_commit".into(),
                    name: None,
                    parent: None,
                    fields: serde_json::json!({"source_node":"extract", "agent_id":"{{trigger.output.agent_id}}"})
                        .as_object()
                        .unwrap()
                        .clone(),
                    position: None,
                },
            ],
            edges: vec![
                crate::workflow::Edge {
                    from: "trigger".into(),
                    from_port: "out".into(),
                    to: "extract".into(),
                    to_port: "in".into(),
                },
                crate::workflow::Edge {
                    from: "extract".into(),
                    from_port: "out".into(),
                    to: "commit".into(),
                    to_port: "in".into(),
                },
            ],
            body: String::new(),
        };
        let exec = MockExecutor::new().with_task_output("extract", payload.clone());

        let state = Engine::start_with_id(
            &blueprint,
            "run-memory",
            serde_json::json!({"agent_id":"agent-a"}),
            dir.path(),
            &exec,
        )
        .await
        .unwrap();

        assert_eq!(state.status, RunStatus::Completed);
        assert_eq!(state.node_output("commit"), Some(&payload));
        assert_eq!(exec.calls(), vec!["task:extract", "memory_commit:commit"]);
    }

    #[tokio::test]
    async fn memory_commit_rejects_a_model_output_principal() {
        let dir = tempfile::tempdir().unwrap();
        let blueprint = Blueprint {
            schema: 2,
            id: "spoofed-memory-workflow".into(),
            name: "Spoofed memory workflow".into(),
            nodes: vec![
                Node {
                    id: "trigger".into(),
                    r#type: "manual_trigger".into(),
                    name: None,
                    parent: None,
                    fields: serde_json::Map::new(),
                    position: None,
                },
                Node {
                    id: "extract".into(),
                    r#type: "task".into(),
                    name: None,
                    parent: None,
                    fields: serde_json::json!({"agent":"role:curator","prompt":"extract"})
                        .as_object()
                        .unwrap()
                        .clone(),
                    position: None,
                },
                Node {
                    id: "commit".into(),
                    r#type: "memory_commit".into(),
                    name: None,
                    parent: None,
                    fields: serde_json::json!({
                        "source_node":"extract",
                        "agent_id":"{{nodes.extract.output.agent_id}}"
                    })
                    .as_object()
                    .unwrap()
                    .clone(),
                    position: None,
                },
            ],
            edges: vec![
                crate::workflow::Edge {
                    from: "trigger".into(),
                    from_port: "out".into(),
                    to: "extract".into(),
                    to_port: "in".into(),
                },
                crate::workflow::Edge {
                    from: "extract".into(),
                    from_port: "out".into(),
                    to: "commit".into(),
                    to_port: "in".into(),
                },
            ],
            body: String::new(),
        };
        let exec = MockExecutor::new().with_task_output(
            "extract",
            serde_json::json!({"agent_id":"agent-b", "operations": []}),
        );

        let state = Engine::start_with_id(
            &blueprint,
            "run-spoofed-memory",
            serde_json::json!({"agent_id":"agent-a"}),
            dir.path(),
            &exec,
        )
        .await
        .unwrap();

        assert_eq!(state.status, RunStatus::Failed);
        assert_eq!(exec.calls(), vec!["task:extract"]);
        assert!(state
            .failure
            .as_deref()
            .is_some_and(|failure| failure.contains("invocation-owned")));
    }
}
