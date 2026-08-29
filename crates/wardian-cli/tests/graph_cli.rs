use std::process::Command;
use tempfile::TempDir;
use wardian_core::control::{
    InteractionBodyRef, InteractionKind, InteractionRecord, InteractionStatus,
    InteractionTriggerPolicy,
};
use wardian_core::db::{
    run_migrations, upsert_agent_with_conn, upsert_interaction_record_with_conn, AgentUpsert,
};
use wardian_core::topology::{save_topology, Topology};

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

fn seed_agent(conn: &rusqlite::Connection, uuid: &str, name: &str, workspace: &str) {
    upsert_agent_with_conn(
        conn,
        &AgentUpsert {
            session_id: uuid,
            session_name: name,
            description: "",
            agent_class: "Coder",
            provider: "codex",
            workspace: Some(workspace),
            project: Some("Wardian"),
            is_off: false,
            created_at: Some("2026-07-03T10:00:00.000Z"),
        },
    )
    .unwrap();
}

fn message_record(id: &str, sender: &str, target: &str, created_at: &str) -> InteractionRecord {
    InteractionRecord {
        id: id.to_string(),
        kind: InteractionKind::Message,
        sender_session_id: Some(sender.to_string()),
        target_session_ids: vec![target.to_string()],
        status: InteractionStatus::Completed,
        trigger_policy: InteractionTriggerPolicy::NotifyOnly,
        body_ref: InteractionBodyRef::Inline { body: "hi".into() },
        parent_interaction_id: None,
        created_at: created_at.to_string(),
        updated_at: created_at.to_string(),
        completed_at: None,
    }
}

/// Three agents; one manual edge uuid-1<->uuid-2; traffic uuid-1<->uuid-3 (unmapped).
fn seed_home() -> TempDir {
    let dir = TempDir::new().unwrap();
    let conn = rusqlite::Connection::open(dir.path().join("state.db")).unwrap();
    run_migrations(&conn).unwrap();
    seed_agent(&conn, "uuid-1", "coder-a1", "D:/ws");
    seed_agent(&conn, "uuid-2", "architect-a1", "D:/ws");
    seed_agent(&conn, "uuid-3", "fork-coder", "D:/other");
    upsert_interaction_record_with_conn(
        &conn,
        &message_record("int_1", "uuid-1", "uuid-3", "2026-07-03T09:00:00Z"),
    )
    .unwrap();

    let mut topology = Topology::default();
    topology.add_edge("uuid-1", "uuid-2", "2026-07-03T08:00:00Z");
    save_topology(dir.path(), &topology).unwrap();
    dir
}

fn run_graph(home: &TempDir, session: Option<&str>, args: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(bin());
    cmd.arg("graph").args(args).env("WARDIAN_HOME", home.path());
    match session {
        Some(session_id) => cmd.env("WARDIAN_SESSION_ID", session_id),
        None => cmd.env_remove("WARDIAN_SESSION_ID"),
    };
    cmd.output().unwrap()
}

fn stdout_json(output: &std::process::Output) -> serde_json::Value {
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn show_returns_agents_edges_unmapped_and_ignored() {
    let home = seed_home();
    let body = stdout_json(&run_graph(&home, None, &["show"]));

    assert_eq!(body["schema"], 1);
    assert_eq!(body["agents"].as_array().unwrap().len(), 3);
    let edges = body["edges"].as_array().unwrap();
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0]["a"], "uuid-1");
    assert_eq!(edges[0]["b"], "uuid-2");
    let unmapped = body["unmapped_pairs"].as_array().unwrap();
    assert_eq!(unmapped.len(), 1);
    assert_eq!(unmapped[0]["a"], "uuid-1");
    assert_eq!(unmapped[0]["b"], "uuid-3");
    assert_eq!(body["ignored_pairs"].as_array().unwrap().len(), 0);
}

#[test]
fn show_excludes_ignored_pairs_from_unmapped() {
    let home = seed_home();
    let mut topology = Topology::default();
    topology.add_edge("uuid-1", "uuid-2", "2026-07-03T08:00:00Z");
    topology.ignore_pair("uuid-1", "uuid-3");
    save_topology(home.path(), &topology).unwrap();

    let body = stdout_json(&run_graph(&home, None, &["show"]));

    assert_eq!(body["unmapped_pairs"].as_array().unwrap().len(), 0);
    assert_eq!(body["ignored_pairs"].as_array().unwrap().len(), 1);
}

#[test]
fn show_reconciles_dangling_references_and_preserves_known_topology() {
    let home = seed_home();
    let mut topology = Topology::default();
    topology.add_edge("uuid-1", "uuid-2", "2026-07-03T08:00:00Z");
    topology.add_edge("uuid-1", "missing-agent", "2026-07-03T08:01:00Z");
    topology.ignore_pair("uuid-1", "uuid-2");
    topology.ignore_pair("uuid-2", "missing-agent");
    topology
        .suppressed_seed_pairs
        .push(wardian_core::topology::IgnoredPair {
            a: "uuid-1".into(),
            b: "uuid-2".into(),
        });
    topology
        .suppressed_seed_pairs
        .push(wardian_core::topology::IgnoredPair {
            a: "uuid-1".into(),
            b: "missing-agent".into(),
        });
    save_topology(home.path(), &topology).unwrap();

    let body = stdout_json(&run_graph(&home, None, &["show"]));

    assert_eq!(body["edges"].as_array().unwrap().len(), 1);
    assert_eq!(body["edges"][0]["a"], "uuid-1");
    assert_eq!(body["edges"][0]["b"], "uuid-2");
    assert_eq!(body["ignored_pairs"].as_array().unwrap().len(), 1);
    assert_eq!(body["ignored_pairs"][0]["a"], "uuid-1");
    assert_eq!(body["ignored_pairs"][0]["b"], "uuid-2");

    let saved = load_topology(home.path());
    assert_eq!(saved.edges.len(), 1);
    assert_eq!(saved.ignored_pairs.len(), 1);
    assert_eq!(saved.suppressed_seed_pairs.len(), 1);
    assert!(saved.is_seed_suppressed("uuid-1", "uuid-2"));
}

#[test]
fn neighbors_defaults_to_self_in_session() {
    let home = seed_home();
    let body = stdout_json(&run_graph(&home, Some("uuid-1"), &["neighbors"]));

    assert_eq!(body["agent_uuid"], "uuid-1");
    let members = body["members"].as_array().unwrap();
    assert_eq!(members.len(), 1);
    assert_eq!(members[0]["uuid"], "uuid-2");
    assert_eq!(members[0]["name"], "architect-a1");
    assert_eq!(members[0]["reasons"][0], "manual");
}

#[test]
fn neighbors_reports_workspace_fallback_for_edgeless_agent() {
    let home = seed_home();
    // uuid-3 has no manual edges; fallback engages but no other agent shares D:/other.
    let body = stdout_json(&run_graph(&home, None, &["neighbors", "fork-coder"]));
    assert_eq!(body["members"].as_array().unwrap().len(), 0);

    // architect-a1 has a manual edge to coder-a1 only.
    let body = stdout_json(&run_graph(&home, None, &["neighbors", "architect-a1"]));
    let members = body["members"].as_array().unwrap();
    assert_eq!(members.len(), 1);
    assert_eq!(members[0]["uuid"], "uuid-1");
}

#[test]
fn neighbors_without_session_or_arg_exits_three() {
    let home = seed_home();
    let output = run_graph(&home, None, &["neighbors"]);
    assert_eq!(output.status.code(), Some(3));
}

#[test]
fn activity_flags_unmapped_pairs() {
    let home = seed_home();
    let body = stdout_json(&run_graph(&home, None, &["activity"]));

    let pairs = body["pairs"].as_array().unwrap();
    assert_eq!(pairs.len(), 1);
    assert_eq!(pairs[0]["a"], "uuid-1");
    assert_eq!(pairs[0]["b"], "uuid-3");
    assert_eq!(pairs[0]["last_message_at"], "2026-07-03T09:00:00Z");
    assert_eq!(pairs[0]["active_ask"], false);
    assert_eq!(pairs[0]["unmapped"], true);
}

#[test]
fn show_pretty_is_human_readable() {
    let home = seed_home();
    let output = run_graph(&home, None, &["show", "--pretty"]);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("coder-a1 <-> architect-a1"));
    assert!(stdout.contains("unmapped"));
}

use wardian_core::topology::load_topology;

// Mutations (link/unlink/ignore/unignore) now route through the control
// plane: topology.json's sole writer is the running Wardian app, and it is
// the sole authority on whether a caller may edit a given pair (see
// crates/wardian-core/src/topology.rs's `authorize_topology_mutation_v1` and
// crate::commands::topology::dispatch_topology_mutation in src-tauri). These
// CLI-subprocess tests can no longer exercise a real mutation or the
// self-serve/stale-session authorization decision, because there is no
// running app for the CLI to connect to here — the same reason
// `agent delete` is only tested for its `app_not_running` path at this layer
// (see forced_delete_without_app_running_exits_six in agent_cli.rs).
//
// What's covered where:
// - Argument/name resolution and other purely local validation: below.
// - Authorization (self-serve, stale session, operator, team coordinator)
//   and the #1032 unlink/team-reseed regression: src-tauri/src/control.rs's
//   `topology_control_*` tests, which exercise the exact same dispatcher
//   function through an in-process mock app.
// - End-to-end CLI mutation against a real running app:
//   e2e-native/tests/topology-cli-native.test.mjs.

#[test]
fn link_unknown_agent_exits_two() {
    let home = seed_home();
    let output = run_graph(&home, Some("uuid-1"), &["link", "ghost"]);
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn link_self_exits_one() {
    let home = seed_home();
    let output = run_graph(&home, None, &["link", "uuid-2", "uuid-2"]);
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn link_outside_session_requires_two_args() {
    let home = seed_home();
    let output = run_graph(&home, None, &["link", "uuid-2"]);
    assert_eq!(output.status.code(), Some(1));
    // Nothing was written; the missing-arg error is purely local.
    assert_eq!(load_topology(home.path()).edges.len(), 1);
}

#[test]
fn stale_session_fails_closed_without_reaching_the_app() {
    let home = seed_home();
    let output = run_graph(&home, Some("uuid-gone"), &["link", "uuid-2", "uuid-3"]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains(r#""code":"not_found""#), "stderr: {stderr}");
    // Nothing was written.
    assert_eq!(load_topology(home.path()).edges.len(), 1);
}

#[test]
fn link_without_running_app_reports_app_not_running() {
    let home = seed_home();
    let output = run_graph(&home, Some("uuid-1"), &["link", "fork-coder"]);
    assert_eq!(output.status.code(), Some(6));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains(r#""code":"app_not_running""#),
        "stderr: {stderr}"
    );
    assert_eq!(load_topology(home.path()).edges.len(), 1);
}

#[test]
fn unlink_without_running_app_reports_app_not_running() {
    let home = seed_home();
    let output = run_graph(&home, Some("uuid-1"), &["unlink", "architect-a1"]);
    assert_eq!(output.status.code(), Some(6));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains(r#""code":"app_not_running""#),
        "stderr: {stderr}"
    );
    assert_eq!(load_topology(home.path()).edges.len(), 1);
}

#[test]
fn ignore_without_running_app_reports_app_not_running() {
    let home = seed_home();
    let output = run_graph(&home, Some("uuid-1"), &["ignore", "fork-coder"]);
    assert_eq!(output.status.code(), Some(6));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains(r#""code":"app_not_running""#),
        "stderr: {stderr}"
    );
    assert!(!load_topology(home.path()).is_ignored("uuid-1", "uuid-3"));
}

#[test]
fn unignore_without_running_app_reports_app_not_running() {
    let home = seed_home();
    let mut topology = Topology::default();
    topology.add_edge("uuid-1", "uuid-2", "2026-07-03T08:00:00Z");
    topology.ignore_pair("uuid-1", "uuid-3");
    save_topology(home.path(), &topology).unwrap();

    let output = run_graph(&home, Some("uuid-1"), &["unignore", "fork-coder"]);
    assert_eq!(output.status.code(), Some(6));
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains(r#""code":"app_not_running""#),
        "stderr: {stderr}"
    );
    assert!(load_topology(home.path()).is_ignored("uuid-1", "uuid-3"));
}
