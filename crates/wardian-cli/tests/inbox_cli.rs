use std::fs;
use std::process::Command;

use serde_json::Value;
use tempfile::TempDir;
use wardian_core::control::{
    InboxNotificationKind, InboxNotificationPayload, InteractionBodyRef, InteractionKind,
    InteractionRecord, InteractionStatus, InteractionTriggerPolicy,
};
use wardian_core::db::{
    run_migrations, upsert_agent_with_conn, upsert_interaction_record_with_conn, AgentUpsert,
};
use wardian_core::engine::{
    store::append_event, store::write_checkpoint, Event, EventKind, RunState, RunStatus,
};

fn bin() -> std::path::PathBuf {
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_wardian-cli") {
        return path.into();
    }
    if let Ok(path) = std::env::var("CARGO_BIN_EXE_wardian_cli") {
        return path.into();
    }

    let exe = if cfg!(windows) {
        "wardian-cli.exe"
    } else {
        "wardian-cli"
    };
    std::env::current_exe()
        .unwrap()
        .parent()
        .and_then(|deps| deps.parent())
        .unwrap()
        .join(exe)
}

fn seed_queue() -> TempDir {
    let home = TempDir::new().unwrap();
    let queue_dir = home.path().join("queue");
    fs::create_dir_all(&queue_dir).unwrap();
    let now = chrono::Utc::now().timestamp_millis();
    let items = serde_json::json!([
        {
            "id": "old-completion",
            "type": "agent_completed",
            "timestamp": now - 8 * 24 * 60 * 60 * 1000,
            "read": true,
            "evidence_source": "provider_runtime"
        },
        {
            "id": "new-update",
            "type": "action_needed",
            "timestamp": now - 1_000,
            "read": false,
            "evidence_source": "provider_runtime",
            "summary": "Provider needs a selection"
        },
        {
            "id": "middle-approval",
            "type": "approval_request",
            "timestamp": now - 2_000,
            "read": false,
            "evidence_source": "interaction_store",
            "dismissed": false,
            "summary": "Review deployment"
        }
    ]);
    fs::write(
        queue_dir.join("items.json"),
        serde_json::to_vec_pretty(&items).unwrap(),
    )
    .unwrap();
    home
}

fn seed_queue_with_empty_db() -> TempDir {
    let home = seed_queue();
    fs::File::create(home.path().join("state.db")).unwrap();
    home
}

fn seed_notification() -> TempDir {
    let home = TempDir::new().unwrap();
    let conn = rusqlite::Connection::open(home.path().join("state.db")).unwrap();
    run_migrations(&conn).unwrap();
    upsert_agent_with_conn(
        &conn,
        &AgentUpsert {
            session_id: "agent-1",
            session_name: "coder-a1",
            description: "",
            agent_class: "Coder",
            provider: "mock",
            workspace: None,
            project: None,
            is_off: false,
            created_at: Some("2026-08-28T10:00:00.000Z"),
        },
    )
    .unwrap();
    let payload = InboxNotificationPayload {
        kind: InboxNotificationKind::Update,
        title: "Migration result".to_string(),
        body: "The migration passed.".to_string(),
        proposed_action: None,
        risk: None,
        choices: Vec::new(),
        expires_at: None,
    };
    let record = InteractionRecord {
        id: "notify-1".to_string(),
        kind: InteractionKind::Notification,
        sender_session_id: Some("agent-1".to_string()),
        target_session_ids: Vec::new(),
        status: InteractionStatus::Completed,
        trigger_policy: InteractionTriggerPolicy::NotifyOnly,
        body_ref: InteractionBodyRef::Inline {
            body: serde_json::to_string(&payload).unwrap(),
        },
        parent_interaction_id: None,
        created_at: "2026-08-28T10:01:00.000Z".to_string(),
        updated_at: "2026-08-28T10:01:00.000Z".to_string(),
        completed_at: Some("2026-08-28T10:01:00.000Z".to_string()),
    };
    upsert_interaction_record_with_conn(&conn, &record).unwrap();
    home
}

fn seed_workflow_approval() -> TempDir {
    let home = TempDir::new().unwrap();
    let blueprint_path = home.path().join("library/workflows/deploy.md");
    fs::create_dir_all(blueprint_path.parent().unwrap()).unwrap();
    fs::write(
        &blueprint_path,
        r#"---
schema: 2
id: deploy
name: Deploy
nodes:
  - id: approve
    type: approval
    name: Deploy production
    fields:
      prompt: Approve the production deployment?
edges: []
---

# Deploy
"#,
    )
    .unwrap();

    let run_root = home.path().join("logs/workflows/deploy/run-1");
    let mut state = RunState::new("run-1", "deploy");
    state.status = RunStatus::AwaitingApproval;
    write_checkpoint(&run_root, &state).unwrap();
    append_event(
        &run_root,
        &Event::at(
            0,
            "2026-08-28T10:02:00.000Z".to_string(),
            EventKind::AwaitingApproval {
                node: "approve".to_string(),
            },
        ),
    )
    .unwrap();
    home
}

fn seed_terminal_workflows() -> TempDir {
    let home = TempDir::new().unwrap();
    for (blueprint_id, run_id, status, event) in [
        (
            "completed-workflow",
            "run-completed",
            RunStatus::Completed,
            EventKind::RunCompleted,
        ),
        (
            "failed-workflow",
            "run-failed",
            RunStatus::Failed,
            EventKind::RunFailed {
                error: "provider exited".to_string(),
            },
        ),
    ] {
        let run_root = home
            .path()
            .join("logs/workflows")
            .join(blueprint_id)
            .join(run_id);
        let mut state = RunState::new(run_id, blueprint_id);
        state.status = status;
        if status == RunStatus::Failed {
            state.failure = Some("provider exited".to_string());
        }
        write_checkpoint(&run_root, &state).unwrap();
        append_event(
            &run_root,
            &Event::at(
                0,
                "2026-08-28T10:03:00.000Z".to_string(),
                EventKind::NodeCompleted {
                    node: "finish".to_string(),
                    output: serde_json::json!({ "text": "Workflow result" }),
                },
            ),
        )
        .unwrap();
        append_event(
            &run_root,
            &Event::at(1, "2026-08-28T10:04:00.000Z".to_string(), event),
        )
        .unwrap();
    }
    home
}

#[test]
fn list_reads_persisted_inbox_and_applies_type_source_and_unread_filters() {
    let home = seed_queue();
    let output = Command::new(bin())
        .args([
            "inbox",
            "list",
            "--type",
            "action_needed,approval_request",
            "--source",
            "provider_runtime,interaction_store",
            "--unread",
        ])
        .env("WARDIAN_HOME", home.path())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["schema"], 1);
    assert_eq!(response["items"].as_array().unwrap().len(), 2);
    assert_eq!(response["items"][0]["id"], "new-update");
    assert_eq!(response["items"][1]["id"], "middle-approval");
    assert_eq!(response["truncated"], false);
    assert_eq!(response["next_offset"], Value::Null);
}

#[test]
fn list_paginates_after_filtering() {
    let home = seed_queue();
    let output = Command::new(bin())
        .args(["inbox", "list", "--unread", "--limit", "1", "--offset", "1"])
        .env("WARDIAN_HOME", home.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["items"].as_array().unwrap().len(), 1);
    assert_eq!(response["items"][0]["id"], "middle-approval");
    assert_eq!(response["truncated"], false);
}

#[test]
fn list_falls_back_to_queue_when_existing_db_is_unmigrated() {
    let home = seed_queue_with_empty_db();
    let output = Command::new(bin())
        .args(["inbox", "list", "--type", "action_needed"])
        .env("WARDIAN_HOME", home.path())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["items"][0]["id"], "new-update");
}

#[test]
fn list_rejects_unbounded_offsets() {
    let home = seed_queue();
    let output = Command::new(bin())
        .args(["inbox", "list", "--offset", "100001"])
        .env("WARDIAN_HOME", home.path())
        .output()
        .unwrap();

    assert!(!output.status.success());
    let response: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(response["error"]["code"], "invalid_offset");
}

#[test]
fn list_rejects_offsets_that_cannot_return_a_usable_cursor() {
    let home = seed_queue();
    let output = Command::new(bin())
        .args(["inbox", "list", "--offset", "100000", "--limit", "2"])
        .env("WARDIAN_HOME", home.path())
        .output()
        .unwrap();

    assert!(!output.status.success());
    let response: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(response["error"]["code"], "invalid_offset");
}

#[test]
fn list_ignores_legacy_items_older_than_seven_days() {
    let home = seed_queue();
    let output = Command::new(bin())
        .args(["inbox", "list", "--type", "agent_completed"])
        .env("WARDIAN_HOME", home.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(response["items"].as_array().unwrap().is_empty());
}

#[test]
fn list_reads_durable_notify_records_with_the_interaction_source() {
    let home = seed_notification();
    let output = Command::new(bin())
        .args([
            "inbox",
            "list",
            "--type",
            "agent_update",
            "--source",
            "interaction_store",
        ])
        .env("WARDIAN_HOME", home.path())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["items"].as_array().unwrap().len(), 1);
    assert_eq!(response["items"][0]["id"], "notification:notify-1");
    assert_eq!(response["items"][0]["agent_name"], "coder-a1");
    assert_eq!(response["items"][0]["summary"], "The migration passed.");
}

#[test]
fn list_includes_persisted_workflow_approvals_when_the_app_is_offline() {
    let home = seed_workflow_approval();
    let output = Command::new(bin())
        .args([
            "inbox",
            "list",
            "--type",
            "approval_request",
            "--source",
            "live_runtime",
        ])
        .env("WARDIAN_HOME", home.path())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["items"].as_array().unwrap().len(), 1);
    assert_eq!(
        response["items"][0]["id"],
        "workflow-approval:deploy:run-1:approve"
    );
    assert_eq!(response["items"][0]["workflow_name"], "Deploy production");
    assert_eq!(
        response["items"][0]["summary"],
        "Approve the production deployment?"
    );
}

#[test]
fn list_includes_durable_terminal_workflow_runs_and_failed_filter_alias() {
    let home = seed_terminal_workflows();
    let output = Command::new(bin())
        .args([
            "inbox",
            "list",
            "--type",
            "workflow_failed",
            "--source",
            "live_runtime",
        ])
        .env("WARDIAN_HOME", home.path())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["items"].as_array().unwrap().len(), 1);
    assert_eq!(
        response["items"][0]["id"],
        "workflow-completion:failed-workflow:run-failed"
    );
    assert_eq!(response["items"][0]["status"], "failed");
    assert_eq!(response["items"][0]["error"], "provider exited");
}

#[test]
fn list_keeps_durable_sources_available_when_the_legacy_queue_is_malformed() {
    let home = seed_notification();
    let queue_dir = home.path().join("queue");
    fs::create_dir_all(&queue_dir).unwrap();
    fs::write(queue_dir.join("items.json"), b"not-json").unwrap();

    let output = Command::new(bin())
        .args(["inbox", "list", "--source", "interaction_store"])
        .env("WARDIAN_HOME", home.path())
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["items"].as_array().unwrap().len(), 1);
    assert_eq!(response["items"][0]["id"], "notification:notify-1");
}
