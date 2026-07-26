use fs2::FileExt;
use once_cell::sync::Lazy;
use std::{
    fs::{File, OpenOptions},
    io,
    sync::Mutex,
};

#[derive(Default)]
struct ExecutionLockState {
    execution_count: usize,
    mutation_in_progress: bool,
}

/// Mirrors the file lock inside one Wardian process. The OS lock coordinates
/// separate processes; this state closes same-process advisory-lock behavior.
static EXECUTION_LOCK_STATE: Lazy<Mutex<ExecutionLockState>> =
    Lazy::new(|| Mutex::new(ExecutionLockState::default()));

/// Held for the duration of a headless provider execution. Worktree deletion
/// takes the exclusive counterpart before it asks Git to remove a managed
/// directory.
///
/// This lock is deliberately global to a Wardian home rather than keyed to the
/// caller's declared workspace. A workflow can dispatch a registered agent or
/// temporary provider in a different assigned workspace, and direct offline
/// delivery runs against the target agent's workspace. A narrower lock would
/// let deletion race a provider's actual working directory.
///
/// The module and on-disk lock path retain their workflow names so a newly
/// started Wardian process still coordinates with already-running versions
/// that only acquire this guard for workflow drives.
pub struct HeadlessExecutionGuard {
    file: File,
}

impl Drop for HeadlessExecutionGuard {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
        release_execution_slot();
    }
}

/// Held while a destructive managed-worktree operation executes. Its exclusive
/// file lock prevents headless provider work in another Wardian process from
/// starting while Git removes a directory.
pub struct WorktreeMutationGuard {
    file: File,
}

impl Drop for WorktreeMutationGuard {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
        release_mutation_slot();
    }
}

/// Takes a shared, non-blocking lock for headless provider execution. If a
/// worktree deletion is already in progress, the caller fails safely rather
/// than beginning provider work in a directory being removed.
pub fn acquire_headless_execution_guard() -> Result<HeadlessExecutionGuard, String> {
    reserve_execution_slot()?;

    let file = match open_lock_file() {
        Ok(file) => file,
        Err(error) => {
            release_execution_slot();
            return Err(error);
        }
    };
    if let Err(error) = FileExt::try_lock_shared(&file) {
        release_execution_slot();
        if lock_is_contended(&error) {
            return Err(
                "headless execution is blocked while a managed worktree is being deleted"
                    .to_string(),
            );
        }
        return Err(format!("failed to lock headless execution: {error}"));
    }

    Ok(HeadlessExecutionGuard { file })
}

/// Attempts the exclusive counterpart used for managed-worktree deletion.
/// `Ok(None)` means an active headless execution is using one or more provider
/// workspaces, so callers must reject destructive removal rather than trying
/// to infer which assigned workspace is active.
pub fn try_acquire_worktree_mutation_guard() -> Result<Option<WorktreeMutationGuard>, String> {
    if !reserve_mutation_slot()? {
        return Ok(None);
    }

    let file = match open_lock_file() {
        Ok(file) => file,
        Err(error) => {
            release_mutation_slot();
            return Err(error);
        }
    };
    if let Err(error) = FileExt::try_lock_exclusive(&file) {
        release_mutation_slot();
        if lock_is_contended(&error) {
            return Ok(None);
        }
        return Err(format!("failed to lock managed worktree deletion: {error}"));
    }

    Ok(Some(WorktreeMutationGuard { file }))
}

fn open_lock_file() -> Result<File, String> {
    let home = crate::paths::wardian_home()
        .ok_or_else(|| "failed to resolve Wardian home for headless execution lock".to_string())?;
    let path = home.join("runtime").join("workflow-execution.lock");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            format!("failed to create headless execution lock directory: {error}")
        })?;
    }
    OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path)
        .map_err(|error| format!("failed to open headless execution lock: {error}"))
}

fn reserve_execution_slot() -> Result<(), String> {
    let mut state = EXECUTION_LOCK_STATE
        .lock()
        .map_err(|_| "headless execution lock state is poisoned".to_string())?;
    if state.mutation_in_progress {
        return Err("headless execution is blocked by a managed worktree deletion".to_string());
    }
    state.execution_count += 1;
    Ok(())
}

fn release_execution_slot() {
    let Ok(mut state) = EXECUTION_LOCK_STATE.lock() else {
        return;
    };
    state.execution_count = state.execution_count.saturating_sub(1);
}

fn reserve_mutation_slot() -> Result<bool, String> {
    let mut state = EXECUTION_LOCK_STATE
        .lock()
        .map_err(|_| "headless execution lock state is poisoned".to_string())?;
    if state.execution_count > 0 || state.mutation_in_progress {
        return Ok(false);
    }
    state.mutation_in_progress = true;
    Ok(true)
}

fn release_mutation_slot() {
    let Ok(mut state) = EXECUTION_LOCK_STATE.lock() else {
        return;
    };
    state.mutation_in_progress = false;
}

fn lock_is_contended(error: &io::Error) -> bool {
    if error.kind() == io::ErrorKind::WouldBlock {
        return true;
    }
    #[cfg(windows)]
    {
        matches!(error.raw_os_error(), Some(32 | 33))
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    struct TestWardianHome {
        _lock: std::sync::MutexGuard<'static, ()>,
        previous_home: Option<OsString>,
        _home: tempfile::TempDir,
    }

    impl TestWardianHome {
        fn new() -> Self {
            let lock = crate::tests::env_lock();
            let home = tempfile::tempdir().expect("temp Wardian home");
            let previous_home = std::env::var_os("WARDIAN_HOME");
            std::env::set_var("WARDIAN_HOME", home.path());
            Self {
                _lock: lock,
                previous_home,
                _home: home,
            }
        }
    }

    impl Drop for TestWardianHome {
        fn drop(&mut self) {
            match self.previous_home.take() {
                Some(value) => std::env::set_var("WARDIAN_HOME", value),
                None => std::env::remove_var("WARDIAN_HOME"),
            }
        }
    }

    #[test]
    fn worktree_mutation_is_rejected_while_any_headless_execution_runs() {
        let _home = TestWardianHome::new();
        let execution = acquire_headless_execution_guard().expect("execution");

        assert!(try_acquire_worktree_mutation_guard()
            .expect("mutation lock")
            .is_none());

        drop(execution);
        assert!(try_acquire_worktree_mutation_guard()
            .expect("mutation lock after execution")
            .is_some());
    }

    #[test]
    fn worktree_mutation_lock_blocks_another_wardian_process() {
        const CHILD_ENV: &str = "WARDIAN_TEST_WORKFLOW_LOCK_CHILD";
        const READY_ENV: &str = "WARDIAN_TEST_WORKFLOW_LOCK_READY";
        const TEST_NAME: &str =
            "workflow_execution_lock::tests::worktree_mutation_lock_blocks_another_wardian_process";

        if std::env::var_os(CHILD_ENV).is_some() {
            let ready = std::env::var_os(READY_ENV)
                .map(std::path::PathBuf::from)
                .expect("child ready marker");
            let _execution = acquire_headless_execution_guard().expect("child execution lock");
            std::fs::write(ready, "ready").expect("child ready marker");
            std::thread::sleep(std::time::Duration::from_millis(250));
            return;
        }

        let home = TestWardianHome::new();
        let ready = tempfile::NamedTempFile::new().expect("ready marker");
        let ready_path = ready.path().to_path_buf();
        drop(ready);

        let mut child =
            std::process::Command::new(std::env::current_exe().expect("current test executable"))
                .arg("--exact")
                .arg(TEST_NAME)
                .env(CHILD_ENV, "1")
                .env("WARDIAN_HOME", home._home.path())
                .env(READY_ENV, &ready_path)
                .spawn()
                .expect("spawn child workflow lock holder");

        for _ in 0..50 {
            if ready_path.exists() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(
            ready_path.exists(),
            "child acquired the headless execution lock"
        );
        assert!(
            try_acquire_worktree_mutation_guard()
                .expect("parent mutation lock")
                .is_none(),
            "a second Wardian process acquired a destructive worktree lock during execution"
        );
        assert!(
            child
                .wait()
                .expect("wait for child workflow lock holder")
                .success(),
            "child workflow lock holder should exit cleanly"
        );
    }
}
