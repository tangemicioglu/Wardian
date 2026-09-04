use std::io::Write;
use std::process::{Command, Output, Stdio};

use serde_json::Value;
use tempfile::TempDir;

fn command(home: &TempDir) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_wardian-cli"));
    command
        .env("WARDIAN_HOME", home.path().join("absent-home"))
        .env_remove("WARDIAN_SESSION_ID");
    command
}

fn json(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn schema_describes_every_command_without_creating_a_home() {
    let home = TempDir::new().unwrap();
    let mut pending = vec![Vec::<String>::new()];
    let mut visited = 0;
    while let Some(path) = pending.pop() {
        let value = json(command(&home).arg("schema").args(&path).output().unwrap());
        assert!(value["usage"]
            .as_str()
            .unwrap()
            .starts_with("Usage: wardian"));
        assert_eq!(value["schema"], 1);
        if let Some(children) = value["commands"].as_array() {
            for child in children {
                let mut next = path.clone();
                next.push(child["name"].as_str().unwrap().to_string());
                pending.push(next);
            }
        }
        visited += 1;
    }
    assert!(
        visited > 100,
        "expected discovery across the full CLI, visited {visited}"
    );
    assert!(!home.path().join("absent-home").exists());
}

#[test]
fn schema_exposes_required_inputs_choices_defaults_and_browser_actions() {
    let home = TempDir::new().unwrap();
    let spawn = json(
        command(&home)
            .args(["schema", "agent", "spawn"])
            .output()
            .unwrap(),
    );
    let args = spawn["args"].as_array().unwrap();
    for name in ["--provider", "--class"] {
        assert_eq!(
            args.iter().find(|arg| arg["name"] == name).unwrap()["required"],
            true
        );
    }
    let list = json(
        command(&home)
            .args(["schema", "agent", "list"])
            .output()
            .unwrap(),
    );
    let scope = list["args"]
        .as_array()
        .unwrap()
        .iter()
        .find(|arg| arg["name"] == "--scope")
        .unwrap();
    assert_eq!(scope["default"], serde_json::json!(["auto"]));
    assert_eq!(
        scope["choices"],
        serde_json::json!(["auto", "neighbors", "workspace", "all"])
    );
    let click = json(
        command(&home)
            .args(["schema", "browser", "<target>", "click"])
            .output()
            .unwrap(),
    );
    assert!(click["args"]
        .as_array()
        .unwrap()
        .iter()
        .any(|arg| arg["name"] == "element_ref" && arg["required"] == true));
}

#[test]
fn selected_node_contract_matches_the_full_registry_and_unknown_nodes_fail() {
    let home = TempDir::new().unwrap();
    let all = json(
        command(&home)
            .args(["automation", "node-types", "--json"])
            .output()
            .unwrap(),
    );
    let task = json(
        command(&home)
            .args(["automation", "node-types", "task", "--json"])
            .output()
            .unwrap(),
    );
    assert_eq!(task["node_types"].as_array().unwrap().len(), 1);
    assert_eq!(
        &task["node_types"][0],
        all["node_types"]
            .as_array()
            .unwrap()
            .iter()
            .find(|node| node["id"] == "task")
            .unwrap()
    );
    let unsupported = json(
        command(&home)
            .args(["automation", "node-types", "sub_automation"])
            .output()
            .unwrap(),
    );
    assert_eq!(unsupported["node_types"][0]["supported"], false);
    let bad = command(&home)
        .args(["automation", "node-types", "typo"])
        .output()
        .unwrap();
    assert_eq!(bad.status.code(), Some(2));
    assert_eq!(
        serde_json::from_slice::<Value>(&bad.stderr).unwrap()["error"]["code"],
        "unknown_node_type"
    );
    assert!(!home.path().join("absent-home").exists());
}

#[test]
fn browser_nested_help_succeeds_without_a_running_browser() {
    let home = TempDir::new().unwrap();
    for args in [
        vec!["browser", "browser:1", "--help"],
        vec!["browser", "browser:1", "snapshot", "--help"],
        vec!["browser", "browser:1", "cookies", "set", "--help"],
        vec!["browser", "browser:1", "storage", "local", "set", "--help"],
    ] {
        let output = command(&home).args(args).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(output.stderr.is_empty());
        assert!(String::from_utf8(output.stdout).unwrap().contains("Usage:"));
    }
    assert!(!home.path().join("absent-home").exists());
}

#[test]
fn invalid_output_selection_fails_before_attempting_a_spawn() {
    let home = TempDir::new().unwrap();
    for args in [
        vec![
            "agent",
            "spawn",
            "--provider",
            "codex",
            "--class",
            "Coder",
            "--fields",
            "typo",
        ],
        vec!["agent", "list", "--fields", ""],
        vec!["agent", "list", "--fields", "typo"],
    ] {
        let output = command(&home).args(args).output().unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stderr).unwrap()["error"]["code"],
            "invalid_field"
        );
    }
    for args in [
        vec!["agent", "list", "--field", "name", "--fields", "status"],
        vec!["agent", "doctor", "any-agent", "--fields", "name"],
        vec!["agent", "list", "--scope", "typo"],
    ] {
        let output = command(&home).args(args).output().unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stderr).unwrap()["error"]["code"],
            "invalid_arguments"
        );
    }
}

#[test]
fn piped_json_is_accepted_before_a_live_launch_and_invalid_input_has_a_hint() {
    let home = TempDir::new().unwrap();
    // A valid payload reaches the live boundary; no automation is created or run.
    for (input, expected) in [
        ("{\"prompt\":\"λ \\\"quote\\\"\"}", "app_not_running"),
        ("[]", "invalid_json"),
    ] {
        let mut child = command(&home)
            .args(["automation", "exec", "missing.md", "--input", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(!output.status.success());
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"]["code"], expected);
    }
}

#[test]
fn equals_form_accepts_empty_configuration_values() {
    let home = TempDir::new().unwrap();
    let output = command(&home)
        .args([
            "agent",
            "update",
            "missing",
            "--description=",
            "--model=",
            "--reasoning-effort=",
        ])
        .output()
        .unwrap();
    // Parsing succeeds with empty strings and reaches the live control boundary.
    assert_eq!(output.status.code(), Some(6));
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stderr).unwrap()["error"]["code"],
        "app_not_running"
    );
}

#[test]
fn conversation_agent_must_exist_but_archived_agents_remain_addressable() {
    let home = TempDir::new().unwrap();
    let unknown = command(&home)
        .args(["conversation", "list", "--agent", "missing"])
        .output()
        .unwrap();
    assert_eq!(unknown.status.code(), Some(2));
    let archive = home
        .path()
        .join("absent-home/agents/archived-agent/conversations/index.jsonl");
    let entry = serde_json::json!({
        "schema": 1, "conversation_id": "conv-1", "agent_id": "archived-agent",
        "agent_name": "Former Agent", "agent_class": "Coder", "workspace": "<workspace>",
        "provider": "codex", "provider_session_ids": [], "started_at": "2026-09-01T00:00:00Z",
        "ended_at": null, "status": "closed", "boundary_reason": "spawn",
        "first_prompt_excerpt": null, "last_record_excerpt": null,
        "record_count": 1, "artifact_count": 0, "path": "conv-1"
    });
    std::fs::create_dir_all(archive.parent().unwrap()).unwrap();
    std::fs::write(archive, format!("{entry}\n")).unwrap();
    let value = json(
        command(&home)
            .args(["conversation", "list", "--agent", "archived-agent"])
            .output()
            .unwrap(),
    );
    assert_eq!(value["status_source"], "persisted");
    assert!(value.to_string().contains("conv-1"));
}

#[test]
fn semantic_validation_failure_is_a_nonzero_structured_error() {
    let home = TempDir::new().unwrap();
    let path = home.path().join("invalid.md");
    std::fs::write(&path, "---\nschema: 2\nid: bad\nname: Bad\nnodes:\n  - id: x\n    type: nonexistent\nedges: []\n---\n").unwrap();
    let output = command(&home)
        .args(["automation", "validate"])
        .arg(path)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["code"], "validation_failed");
    assert_eq!(error["error"]["details"]["ok"], false);
    assert!(!error["error"]["details"]["diagnostics"]
        .as_array()
        .unwrap()
        .is_empty());
}
