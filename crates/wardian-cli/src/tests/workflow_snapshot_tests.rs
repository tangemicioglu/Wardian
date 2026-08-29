#[test]
fn workflow_replay_uses_the_run_snapshot_when_the_library_copy_is_missing() {
    let temp = tempfile::tempdir().unwrap();
    let _env = TestWardianHome::new(temp.path());
    let blueprint = wardian_core::workflow::Blueprint {
        schema: 2,
        id: "snapshot-only".into(),
        name: "Snapshot only".into(),
        nodes: Vec::new(),
        edges: Vec::new(),
        body: String::new(),
    };
    let run_root = workflow_run_root("snapshot-only", "run-1").unwrap();
    wardian_core::engine::Engine::initialize_with_id(
        &blueprint,
        "run-1",
        serde_json::json!({}),
        &run_root,
    )
    .unwrap();
    wardian_core::engine::store::append_event(
        &run_root,
        &wardian_core::engine::event::Event::at(
            1,
            "done".into(),
            wardian_core::engine::event::EventKind::RunCompleted,
        ),
    )
    .unwrap();

    let rendered = workflow_replay::render("snapshot-only", "run-1").unwrap();
    let json: serde_json::Value = serde_json::from_str(&rendered).unwrap();

    assert_eq!(json["state"]["run_id"], "run-1");
    assert_eq!(json["state"]["blueprint_id"], "snapshot-only");
    assert_eq!(json["state"]["status"], "completed");
}

#[test]
fn workflow_replay_supports_a_scheduler_launch_failure_without_a_blueprint() {
    let temp = tempfile::tempdir().unwrap();
    let _env = TestWardianHome::new(temp.path());
    let run_root = workflow_run_root("missing-workflow", "run-1").unwrap();
    let message = "could not resolve blueprint path for missing-workflow";
    let mut state = wardian_core::engine::RunState::new("run-1", "missing-workflow");
    state.status = wardian_core::engine::RunStatus::Failed;
    state.failure = Some(message.into());
    state.next_seq = 1;
    wardian_core::engine::store::write_checkpoint(&run_root, &state).unwrap();
    wardian_core::engine::store::append_event(
        &run_root,
        &wardian_core::engine::Event::new(
            0,
            wardian_core::engine::EventKind::RunFailed {
                error: message.into(),
            },
        ),
    )
    .unwrap();

    let rendered = workflow_replay::render("missing-workflow", "run-1").unwrap();
    let json: serde_json::Value = serde_json::from_str(&rendered).unwrap();

    assert_eq!(json["state"]["run_id"], "run-1");
    assert_eq!(json["state"]["status"], "failed");
    assert_eq!(json["state"]["failure"], message);
}
