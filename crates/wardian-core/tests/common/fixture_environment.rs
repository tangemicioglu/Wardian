//! Exercise the real fixture tests with process-local, unusable log overrides.

#[test]
fn fixture_tests_ignore_ambient_log_overrides() {
    let dir = tempfile::tempdir().unwrap();
    let invalid = dir.path().join("invalid.jsonl");
    std::fs::write(&invalid, "not a provider record\n").unwrap();
    let missing = dir.path().join("missing.jsonl");

    for log in [&invalid, &missing] {
        // Run only the six fixture tests, excluding this subprocess guard.
        // Child-only overrides avoid races with the surrounding test process.
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["the_fixture_", "--nocapture", "--test-threads=1"])
            .env("WARDIAN_TEST_CODEX_LOG", log)
            .env("WARDIAN_TEST_PI_LOG", log)
            .env("WARDIAN_HOME", dir.path())
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "fixture subprocess failed:\n{stdout}\n{stderr}"
        );
        // Pin the retained fixture coverage too: zero selected tests or ignored
        // fixtures must not make the isolation guard vacuously green.
        assert!(
            stdout.contains("6 passed; 0 failed; 0 ignored"),
            "expected all six fixture assertions to run:\n{stdout}\n{stderr}"
        );
    }
}
