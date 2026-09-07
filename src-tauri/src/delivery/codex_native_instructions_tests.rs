//! Command construction only: no provider process or turn is started.
use super::*;

struct TestHome {
    _lock: tokio::sync::MutexGuard<'static, ()>,
    root: tempfile::TempDir,
    prior_home: Option<std::ffi::OsString>,
    prior_script: Option<std::ffi::OsString>,
}

impl TestHome {
    fn new(memory_enabled: bool) -> Self {
        let lock = crate::utils::wardian_test_env_lock();
        let root = tempfile::tempdir().unwrap();
        let prior_home = std::env::var_os("WARDIAN_HOME");
        let prior_script = std::env::var_os("WARDIAN_NATIVE_TEST_SCRIPT");
        std::env::set_var("WARDIAN_HOME", root.path());
        std::env::remove_var("WARDIAN_NATIVE_TEST_SCRIPT");
        std::fs::create_dir_all(root.path().join("settings")).unwrap();
        std::fs::write(
            root.path().join("settings/app.json"),
            serde_json::json!({"schema_version":2,"overrides":{"memory_enabled":memory_enabled}})
                .to_string(),
        )
        .unwrap();
        Self {
            _lock: lock,
            root,
            prior_home,
            prior_script,
        }
    }

    fn spec(&self) -> NativeSessionSpec {
        let workspace = self.root.path().join("workspace");
        for (relative, text) in [
            ("common/AGENTS.md", "COMMON_SENTINEL"),
            ("classes/Builder/AGENTS.md", "CLASS_SENTINEL"),
            ("agents/owner/AGENTS.md", "OWNER_SENTINEL"),
            ("agents/other/AGENTS.md", "OTHER_AGENT_SENTINEL"),
            ("workspace/AGENTS.md", "WORKSPACE_ONLY"),
        ] {
            let path = self.root.path().join(relative);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        NativeSessionSpec {
            target_agent_id: "owner".into(),
            provider: "codex".into(),
            generation: 7,
            workspace: workspace.clone(),
            config: AgentConfig {
                session_id: "owner".into(),
                provider: "codex".into(),
                agent_class: "Builder".into(),
                folder: workspace.to_string_lossy().into(),
                model: Some("configured-model".into()),
                resume_session: Some("configured-thread".into()),
                provider_config: wardian_core::models::ProviderConfig::Codex(
                    wardian_core::models::CodexProviderConfig {
                        reasoning_effort: Some("low".into()),
                        ..Default::default()
                    },
                ),
                ..Default::default()
            },
        }
    }
}

impl Drop for TestHome {
    fn drop(&mut self) {
        for (name, prior) in [
            ("WARDIAN_HOME", self.prior_home.take()),
            ("WARDIAN_NATIVE_TEST_SCRIPT", self.prior_script.take()),
        ] {
            match prior {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
    }
}

#[test]
fn native_codex_command_bridges_owner_context_once_without_changing_resume() {
    for enabled in [false, true] {
        let fixture = TestHome::new(enabled);
        let spec = fixture.spec();
        let store = if enabled {
            let store = wardian_core::memory::MemoryStore::from_default_home().unwrap();
            for (agent, workspace, text) in [
                ("owner", spec.workspace.clone(), "OWNER_MEMORY"),
                ("other", spec.workspace.clone(), "OTHER_MEMORY"),
                (
                    "owner",
                    fixture.root.path().join("elsewhere"),
                    "OTHER_WORKSPACE_MEMORY",
                ),
            ] {
                store
                    .save(
                        &wardian_core::memory::MemoryActor::Operator,
                        wardian_core::memory::SaveMemoryRequest {
                            agent_id: agent.into(),
                            workspace: Some(workspace.to_string_lossy().into()),
                            kind: wardian_core::memory::MemoryKind::Stable,
                            text: text.into(),
                            evidence_excerpt: "Native instruction transport fixture".into(),
                            sources: vec![],
                            idempotency_key: None,
                        },
                    )
                    .unwrap();
            }
            Some(store)
        } else {
            None
        };
        let prior = NativeSessionBinding {
            target_agent_id: "owner".into(),
            generation: 6,
            provider: "codex".into(),
            transport: "codex_app_server".into(),
            provider_session_id: Some("bound-thread".into()),
            capabilities: NativeProviderProtocol::CodexAppServer.capabilities("fixture"),
            observed_at: now(),
        };
        // Constructs the real command only: never spawns the executable.
        let (command, lease) =
            native_command(&spec, NativeProviderProtocol::CodexAppServer, Some(&prior)).unwrap();
        assert_eq!(lease.is_some(), enabled);
        assert_eq!(
            command.as_std().get_current_dir(),
            Some(spec.workspace.as_path())
        );
        let args = command
            .as_std()
            .get_args()
            .map(|arg| arg.to_str().unwrap())
            .collect::<Vec<_>>();
        let overrides = args
            .windows(2)
            .filter(|pair| pair[0] == "-c")
            .map(|pair| pair[1].parse::<toml_edit::DocumentMut>().unwrap())
            .collect::<Vec<_>>();
        let instructions = overrides
            .iter()
            .filter_map(|value| value.get("developer_instructions").and_then(|v| v.as_str()))
            .collect::<Vec<_>>();
        assert_eq!(instructions.len(), 1);
        let text = instructions[0];
        assert!(text.find("COMMON_SENTINEL").unwrap() < text.find("CLASS_SENTINEL").unwrap());
        assert!(text.find("CLASS_SENTINEL").unwrap() < text.find("OWNER_SENTINEL").unwrap());
        for excluded in [
            "OTHER_AGENT_SENTINEL",
            "WORKSPACE_ONLY",
            "OTHER_MEMORY",
            "OTHER_WORKSPACE_MEMORY",
        ] {
            assert!(!text.contains(excluded), "unexpected context: {excluded}");
        }
        assert_eq!(
            text.matches("## Wardian memory").count(),
            usize::from(enabled)
        );
        assert_eq!(text.matches("OWNER_MEMORY").count(), usize::from(enabled));
        // #1199 separately moves --model to a config override. The bridge
        // preserves the configured argument on either side of that change.
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--model", "configured-model"])
                || overrides
                    .iter()
                    .any(|v| v.get("model").and_then(|v| v.as_str()) == Some("configured-model"))
        );
        assert!(overrides
            .iter()
            .any(|v| v.get("model_reasoning_effort").and_then(|v| v.as_str()) == Some("low")));
        assert_eq!(args.last(), Some(&"app-server"));
        assert_eq!(
            spec.config.resume_session.as_deref(),
            Some("configured-thread")
        );
        assert_eq!(prior.provider_session_id.as_deref(), Some("bound-thread"));
        for thread in [None, Some("bound-thread")] {
            let requests = NativeProviderProtocol::CodexAppServer.bootstrap_requests(
                "owner",
                &spec.workspace.to_string_lossy(),
                thread,
            );
            assert_eq!(requests.len(), 2);
            assert_eq!(
                requests[1]["method"],
                if thread.is_some() {
                    "thread/resume"
                } else {
                    "thread/start"
                }
            );
            if let Some(thread) = thread {
                assert_eq!(requests[1]["params"]["threadId"], thread);
            }
            for request in requests {
                assert!(!request.to_string().contains("developerInstructions"));
                assert!(!request.to_string().contains("developer_instructions"));
            }
        }
        assert_eq!(
            std::fs::read_to_string(spec.workspace.join("AGENTS.md")).unwrap(),
            "WORKSPACE_ONLY"
        );
        if let Some(store) = store {
            assert!(store
                .list_events(&wardian_core::memory::MemoryActor::Operator, "owner")
                .unwrap()
                .iter()
                .all(|event| event.action != "loaded"));
        }
    }
}

#[test]
fn native_codex_instruction_failure_is_pre_spawn_and_keeps_binding() {
    let fixture = TestHome::new(true);
    let mut spec = fixture.spec();
    let database = wardian_core::paths::memory_db_path().unwrap();
    std::fs::create_dir_all(&database).unwrap();
    let failure = native_command(&spec, NativeProviderProtocol::CodexAppServer, None)
        .err()
        .expect("memory failure");
    assert_eq!(failure.code, NativeDeliveryErrorCode::TransportUnavailable);
    assert!(!failure.provider_boundary_crossed);
    assert_eq!(
        spec.config.resume_session.as_deref(),
        Some("configured-thread")
    );
    spec.config.session_id = "other".into();
    let failure = native_command(&spec, NativeProviderProtocol::CodexAppServer, None)
        .err()
        .expect("owner mismatch");
    assert!(failure.message.contains("instruction owner"));
    assert!(!failure.provider_boundary_crossed);
}
