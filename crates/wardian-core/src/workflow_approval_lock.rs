use fs2::FileExt;
use once_cell::sync::Lazy;
use std::{
    collections::HashSet,
    fs::{File, OpenOptions},
    io,
    path::{Path, PathBuf},
    sync::Mutex,
};

/// Mirrors per-run file locks within one Wardian process. Advisory file locks
/// alone do not prevent another task in the same process from taking the lock.
static LOCKED_RUNS: Lazy<Mutex<HashSet<PathBuf>>> = Lazy::new(|| Mutex::new(HashSet::new()));

/// Held while a parked workflow run transitions through an approval decision.
///
/// The file lock coordinates separate Wardian processes, while `LOCKED_RUNS`
/// closes the same-process advisory-lock gap.
pub(crate) struct ApprovalDecisionGuard {
    run_root: PathBuf,
    file: File,
}

impl Drop for ApprovalDecisionGuard {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
        release_run(&self.run_root);
    }
}

#[derive(Debug)]
pub(crate) enum ApprovalDecisionLockError {
    Contended,
    Io(io::Error),
}

/// Acquires the exclusive, non-blocking approval-decision guard for one run.
///
/// A contended guard means another caller is already resolving the same parked
/// run. Callers must not read or write its checkpoint in that case.
pub(crate) fn acquire_approval_decision_guard(
    run_root: &Path,
) -> Result<ApprovalDecisionGuard, ApprovalDecisionLockError> {
    std::fs::create_dir_all(run_root).map_err(ApprovalDecisionLockError::Io)?;
    let run_root = run_root
        .canonicalize()
        .map_err(ApprovalDecisionLockError::Io)?;
    reserve_run(&run_root)?;

    let file = match open_lock_file(&run_root) {
        Ok(file) => file,
        Err(error) => {
            release_run(&run_root);
            return Err(ApprovalDecisionLockError::Io(error));
        }
    };
    if let Err(error) = FileExt::try_lock_exclusive(&file) {
        release_run(&run_root);
        if lock_is_contended(&error) {
            return Err(ApprovalDecisionLockError::Contended);
        }
        return Err(ApprovalDecisionLockError::Io(error));
    }

    Ok(ApprovalDecisionGuard { run_root, file })
}

fn reserve_run(run_root: &Path) -> Result<(), ApprovalDecisionLockError> {
    let mut locked_runs = LOCKED_RUNS.lock().map_err(|_| {
        ApprovalDecisionLockError::Io(io::Error::other("workflow approval lock state is poisoned"))
    })?;
    if !locked_runs.insert(run_root.to_path_buf()) {
        return Err(ApprovalDecisionLockError::Contended);
    }
    Ok(())
}

fn release_run(run_root: &Path) {
    let Ok(mut locked_runs) = LOCKED_RUNS.lock() else {
        return;
    };
    locked_runs.remove(run_root);
}

fn open_lock_file(run_root: &Path) -> io::Result<File> {
    OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(run_root.join(".approval-decision.lock"))
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

    #[test]
    fn approval_decision_lock_is_exclusive_per_run() {
        let dir = tempfile::tempdir().expect("temp run directory");
        let first = acquire_approval_decision_guard(dir.path()).expect("first lock");

        assert!(matches!(
            acquire_approval_decision_guard(dir.path()),
            Err(ApprovalDecisionLockError::Contended)
        ));

        drop(first);
        acquire_approval_decision_guard(dir.path()).expect("lock after release");
    }

    #[test]
    fn approval_decision_locks_do_not_block_other_runs() {
        let first = tempfile::tempdir().expect("first run directory");
        let second = tempfile::tempdir().expect("second run directory");
        let _first = acquire_approval_decision_guard(first.path()).expect("first lock");
        let _second = acquire_approval_decision_guard(second.path()).expect("second lock");
    }
}
