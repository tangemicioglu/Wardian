//! Application-owned telemetry retention scheduling.
//!
//! The core maintenance function owns the recovery protocol. This module owns
//! the product policy and the lifecycle boundary that makes invoking it safe:
//! run once a day, only when no Wardian provider runtime is active, and keep a
//! small rotating set of verified backups.

use crate::state::{ActiveAgent, AppState};
use crate::utils::fs::get_wardian_home;
use chrono::Utc;
use std::io;
use std::path::{Path, PathBuf};
use tauri::Manager;
use wardian_core::telemetry::MaintenanceReport;

/// Raw provider facts remain available for this long; hourly rollups remain
/// available beyond it for long-horizon aggregate views.
pub const TELEMETRY_RAW_RETENTION_DAYS: u32 = 90;

const AUTOMATIC_BACKUP_PREFIX: &str = "state.db.telemetry-";
const AUTOMATIC_BACKUP_SUFFIX: &str = ".backup";
const AUTOMATIC_BACKUP_TEMP_SUFFIX: &str = ".tmp";
const PENDING_BACKUP_NAME: &str = "state.db.telemetry-pending.backup";
const AUTOMATIC_BACKUP_COUNT: usize = 2;
const INITIAL_DELAY: std::time::Duration = std::time::Duration::from_secs(60);
const RETRY_DELAY: std::time::Duration = std::time::Duration::from_secs(15 * 60);
const SUCCESS_DELAY: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

fn telemetry_backup_directory(home: &Path) -> PathBuf {
    home.join("backups").join("telemetry")
}

fn new_backup_path(directory: &Path) -> PathBuf {
    directory.join(format!(
        "{AUTOMATIC_BACKUP_PREFIX}{}-{}{AUTOMATIC_BACKUP_SUFFIX}",
        Utc::now().timestamp_millis(),
        std::process::id()
    ))
}

fn pending_backup_path(directory: &Path) -> PathBuf {
    directory.join(PENDING_BACKUP_NAME)
}

fn is_automatic_backup(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.starts_with(AUTOMATIC_BACKUP_PREFIX)
                && name.ends_with(AUTOMATIC_BACKUP_SUFFIX)
                && name != PENDING_BACKUP_NAME
        })
}

fn is_automatic_backup_temp(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.starts_with(AUTOMATIC_BACKUP_PREFIX)
                && name.ends_with(&format!(
                    "{AUTOMATIC_BACKUP_SUFFIX}{AUTOMATIC_BACKUP_TEMP_SUFFIX}"
                ))
        })
}

fn prune_automatic_backups(directory: &Path) -> io::Result<()> {
    remove_incomplete_backup_temps(directory)?;
    let mut backups = Vec::new();
    for entry in std::fs::read_dir(directory)?.flatten() {
        let path = entry.path();
        if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
            continue;
        }
        if is_automatic_backup(&path) {
            backups.push(path);
        }
    }
    backups.sort();

    let remove_count = backups.len().saturating_sub(AUTOMATIC_BACKUP_COUNT);
    for backup in backups.into_iter().take(remove_count) {
        std::fs::remove_file(backup)?;
    }
    Ok(())
}

fn remove_incomplete_backup_temps(directory: &Path) -> io::Result<()> {
    if !directory.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(directory)?.flatten() {
        let path = entry.path();
        if entry.file_type().is_ok_and(|kind| kind.is_file()) && is_automatic_backup_temp(&path) {
            std::fs::remove_file(path)?;
        }
    }
    Ok(())
}

fn verified_pending_backup(directory: &Path) -> io::Result<Option<PathBuf>> {
    let path = pending_backup_path(directory);
    if !path.exists() {
        return Ok(None);
    }
    if wardian_core::telemetry::verify_backup(&path).is_ok() {
        return Ok(Some(path));
    }
    // The pending file is scheduler-owned. It cannot be a recovery baseline
    // if it no longer passes SQLite integrity checks, so allow the next run
    // to create a fresh one at the same stable path.
    std::fs::remove_file(path)?;
    Ok(None)
}

fn backup_contains_prepared_cutoff(path: &Path) -> io::Result<bool> {
    let backup =
        rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .map_err(|error| io::Error::other(error.to_string()))?;
    backup
        .query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM telemetry_meta
                 WHERE key = 'telemetry_maintenance_prepared_cutoff'
             )",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map(|value| value != 0)
        .map_err(|error| io::Error::other(error.to_string()))
}

fn newest_verified_preparation_baseline(directory: &Path) -> io::Result<Option<PathBuf>> {
    if !directory.exists() {
        return Ok(None);
    }
    let mut backups = std::fs::read_dir(directory)?
        .flatten()
        .filter_map(|entry| {
            entry
                .file_type()
                .ok()
                .filter(|kind| kind.is_file())
                .filter(|_| is_automatic_backup(&entry.path()))
                .map(|_| entry.path())
        })
        .collect::<Vec<_>>();
    backups.sort();
    for backup in backups.into_iter().rev() {
        if wardian_core::telemetry::verify_backup(&backup).is_ok()
            && !backup_contains_prepared_cutoff(&backup)?
        {
            return Ok(Some(backup));
        }
    }
    Ok(None)
}

fn promote_pending_backup(directory: &Path) -> io::Result<()> {
    let Some(pending) = verified_pending_backup(directory)? else {
        return Ok(());
    };
    std::fs::rename(pending, new_backup_path(directory))
}

fn remove_pending_backup(directory: &Path) -> io::Result<()> {
    let path = pending_backup_path(directory);
    if path.exists() {
        std::fs::remove_file(path)?;
    }
    Ok(())
}

fn adopt_legacy_baseline(directory: &Path) -> io::Result<PathBuf> {
    let pending = pending_backup_path(directory);
    // A process can terminate after moving a legacy backup here but before
    // core records its durable association. Preserve that verified file so a
    // retry cannot replace the only pre-deletion recovery baseline.
    if verified_pending_backup(directory)?.is_some() {
        return Ok(pending);
    }
    let Some(legacy_baseline) = newest_verified_preparation_baseline(directory)? else {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "no verified pre-preparation telemetry backup is available",
        ));
    };
    std::fs::rename(legacy_baseline, &pending)?;
    Ok(pending)
}

fn agent_runtime_is_active(agent: &ActiveAgent) -> bool {
    agent.runtime_generation.is_some()
        || agent.process_id.is_some()
        || agent.child_process.is_some()
        || !agent.background_processes.is_empty()
}

async fn provider_runtimes_are_quiescent(state: &AppState) -> bool {
    let agents = state.agents.lock().await;
    !agents.values().any(agent_runtime_is_active)
}

fn maintain_if_due_at(
    conn: &rusqlite::Connection,
    home: &Path,
) -> Result<Option<MaintenanceReport>, Box<dyn std::error::Error>> {
    let backup_directory = telemetry_backup_directory(home);
    // A prepared marker is the durable indication that raw rows may already
    // have been deleted. Resume it even when the final expired row is gone;
    // otherwise a checkpoint/vacuum failure after deletion would strand the
    // marker and leave the verified recovery baseline unrotated forever.
    let retention_prepared = wardian_core::telemetry::retention_is_prepared(conn)?;
    let backup_prepared = wardian_core::telemetry::retention_backup_is_prepared(conn)?;
    let has_expired =
        wardian_core::telemetry::has_expired_raw_telemetry(conn, TELEMETRY_RAW_RETENTION_DAYS)?;
    if !retention_prepared && !backup_prepared && !has_expired {
        if backup_directory.exists() {
            // A crash after the core operation cleared its marker but before
            // backup rotation completed leaves one stable pending baseline.
            // Promote it now that there is no active retention phase.
            promote_pending_backup(&backup_directory)?;
            prune_automatic_backups(&backup_directory)?;
        }
        return Ok(None);
    }

    std::fs::create_dir_all(&backup_directory)?;
    remove_incomplete_backup_temps(&backup_directory)?;
    let pending = pending_backup_path(&backup_directory);
    let backup_path = if backup_prepared {
        verified_pending_backup(&backup_directory)?.unwrap_or_else(|| pending.clone())
    } else if retention_prepared {
        adopt_legacy_baseline(&backup_directory)?
    } else {
        // A pending file without its durable association belongs to a
        // completed or abandoned attempt. Do not reuse a baseline that may
        // predate telemetry written since that attempt.
        remove_pending_backup(&backup_directory)?;
        pending.clone()
    };
    let report =
        wardian_core::telemetry::maintain(conn, TELEMETRY_RAW_RETENTION_DAYS, &backup_path, true)?;

    if backup_path == pending {
        if let Err(error) = promote_pending_backup(&backup_directory) {
            crate::utils::logging::log_debug(&format!(
                "[Wardian] Telemetry retention pending backup promotion failed after maintenance: {error}"
            ));
        }
    }
    if let Err(error) = prune_automatic_backups(&backup_directory) {
        crate::utils::logging::log_debug(&format!(
            "[Wardian] Telemetry retention backup rotation failed after maintenance: {error}"
        ));
    }
    Ok(Some(report))
}

fn run_telemetry_maintenance_if_due() -> Result<Option<MaintenanceReport>, String> {
    let Some(home) = get_wardian_home() else {
        return Ok(None);
    };
    wardian_core::db::get_db_conn(|conn| maintain_if_due_at(conn, &home))
        .map_err(|error| error.to_string())
}

/// Start the once-daily application-owned retention loop.
pub fn start_telemetry_maintenance(app_handle: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(INITIAL_DELAY).await;
        loop {
            let state = app_handle.state::<AppState>();
            if !provider_runtimes_are_quiescent(&state).await {
                tokio::time::sleep(RETRY_DELAY).await;
                continue;
            }

            let result = tokio::task::spawn_blocking(run_telemetry_maintenance_if_due).await;
            let retry = match result {
                Ok(Ok(Some(report))) => {
                    crate::utils::logging::log_debug(&format!(
                        "[Wardian] Telemetry retention removed {} turns, {} edits, {} activity intervals, and {} old limit observations before {}",
                        report.turns_deleted,
                        report.edits_deleted,
                        report.activity_deleted,
                        report.limits_deleted,
                        report.cutoff
                    ));
                    false
                }
                Ok(Ok(None)) => false,
                Ok(Err(error)) => {
                    crate::utils::logging::log_debug(&format!(
                        "[Wardian] Telemetry retention pass failed; retrying: {error}"
                    ));
                    true
                }
                Err(error) => {
                    crate::utils::logging::log_debug(&format!(
                        "[Wardian] Telemetry retention task failed; retrying: {error}"
                    ));
                    true
                }
            };

            tokio::time::sleep(if retry { RETRY_DELAY } else { SUCCESS_DELAY }).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};
    use tempfile::tempdir;

    #[test]
    fn automatic_backup_names_are_scoped_to_the_rotation_prefix() {
        assert!(is_automatic_backup(Path::new(
            "state.db.telemetry-1756555200000-42.backup"
        )));
        assert!(!is_automatic_backup(Path::new("state.db.backup")));
        assert!(!is_automatic_backup(Path::new(
            "state.db.telemetry-1756555200000-42.tmp"
        )));
        assert!(!is_automatic_backup(Path::new(PENDING_BACKUP_NAME)));
    }

    #[test]
    fn backup_rotation_keeps_the_two_newest_files() {
        let directory = tempdir().unwrap();
        for suffix in ["100-1", "200-1", "300-1"] {
            std::fs::write(
                directory.path().join(format!(
                    "{AUTOMATIC_BACKUP_PREFIX}{suffix}{AUTOMATIC_BACKUP_SUFFIX}"
                )),
                [],
            )
            .unwrap();
        }

        prune_automatic_backups(directory.path()).unwrap();

        assert!(!directory
            .path()
            .join("state.db.telemetry-100-1.backup")
            .exists());
        assert!(directory
            .path()
            .join("state.db.telemetry-200-1.backup")
            .exists());
        assert!(directory
            .path()
            .join("state.db.telemetry-300-1.backup")
            .exists());
    }

    #[test]
    fn backup_rotation_removes_incomplete_temporary_files() {
        let directory = tempdir().unwrap();
        let temporary = directory.path().join("state.db.telemetry-300-1.backup.tmp");
        std::fs::write(&temporary, []).unwrap();

        prune_automatic_backups(directory.path()).unwrap();

        assert!(!temporary.exists());
    }

    #[test]
    fn due_check_skips_backup_when_no_raw_fact_is_expired() {
        let directory = tempdir().unwrap();
        let database = directory.path().join("state.db");
        let conn = rusqlite::Connection::open(database).unwrap();
        wardian_core::telemetry::run_telemetry_migrations(&conn).unwrap();

        let report = maintain_if_due_at(&conn, directory.path()).unwrap();

        assert!(report.is_none());
        assert!(!telemetry_backup_directory(directory.path()).exists());
    }

    #[test]
    fn due_check_runs_the_application_maintenance_path_for_expired_facts() {
        let directory = tempdir().unwrap();
        let database = directory.path().join("state.db");
        let conn = rusqlite::Connection::open(database).unwrap();
        wardian_core::telemetry::run_telemetry_migrations(&conn).unwrap();
        let old =
            (Utc::now() - Duration::days(TELEMETRY_RAW_RETENTION_DAYS as i64 + 1)).to_rfc3339();
        conn.execute(
            "INSERT INTO telemetry_turns
                (event_key, session_id, provider, ended_at, source_key, source_path)
             VALUES ('old', 'session-a', 'codex', ?1, 'source-a', 'log')",
            [&old],
        )
        .unwrap();

        let report = maintain_if_due_at(&conn, directory.path())
            .unwrap()
            .expect("expired raw telemetry should trigger maintenance");

        assert_eq!(report.turns_deleted, 1);
        assert_eq!(
            conn.query_row::<i64, _, _>("SELECT count(*) FROM telemetry_turns", [], |row| {
                row.get(0)
            })
            .unwrap(),
            0
        );
        let backups = std::fs::read_dir(telemetry_backup_directory(directory.path()))
            .unwrap()
            .count();
        assert_eq!(backups, 1);
    }

    #[test]
    fn due_check_reuses_verified_backup_when_retention_is_in_progress() {
        let directory = tempdir().unwrap();
        let database = directory.path().join("state.db");
        let backup_directory = telemetry_backup_directory(directory.path());
        let baseline = backup_directory.join("state.db.telemetry-100-1.backup");
        let conn = rusqlite::Connection::open(&database).unwrap();
        wardian_core::telemetry::run_telemetry_migrations(&conn).unwrap();
        let now = Utc::now();
        let old =
            (now - chrono::Duration::days(TELEMETRY_RAW_RETENTION_DAYS as i64 + 1)).to_rfc3339();
        let cutoff =
            wardian_core::telemetry::retention_cutoff(now, TELEMETRY_RAW_RETENTION_DAYS).unwrap();
        let cutoff_epoch = chrono::DateTime::parse_from_rfc3339(&cutoff)
            .unwrap()
            .timestamp();
        conn.execute(
            "INSERT INTO telemetry_turns
                (event_key, session_id, provider, ended_at, source_key, source_path)
             VALUES ('old', 'session-a', 'codex', ?1, 'source-a', 'log')",
            [&old],
        )
        .unwrap();
        std::fs::create_dir_all(&backup_directory).unwrap();
        conn.execute(
            "VACUUM INTO ?1",
            rusqlite::params![baseline.to_string_lossy().as_ref()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO telemetry_meta(key, value) VALUES ('telemetry_maintenance_prepared_cutoff', ?1),
                ('telemetry_maintenance_prepared_retain_days', ?2)",
            rusqlite::params![cutoff_epoch, TELEMETRY_RAW_RETENTION_DAYS],
        )
        .unwrap();

        let report = maintain_if_due_at(&conn, directory.path())
            .unwrap()
            .expect("prepared retention should resume");

        assert_eq!(report.turns_deleted, 1);
        assert!(!baseline.exists());
        assert!(!pending_backup_path(&backup_directory).exists());
        assert_eq!(std::fs::read_dir(&backup_directory).unwrap().count(), 1);
    }

    #[test]
    fn legacy_prepared_baseline_stays_pinned_after_post_association_failure() {
        let directory = tempdir().unwrap();
        let database = directory.path().join("state.db");
        let backup_directory = telemetry_backup_directory(directory.path());
        let baseline = backup_directory.join("state.db.telemetry-100-1.backup");
        let conn = rusqlite::Connection::open(&database).unwrap();
        wardian_core::telemetry::run_telemetry_migrations(&conn).unwrap();
        let now = Utc::now();
        let old = (now - Duration::days(TELEMETRY_RAW_RETENTION_DAYS as i64 + 1)).to_rfc3339();
        let cutoff =
            wardian_core::telemetry::retention_cutoff(now, TELEMETRY_RAW_RETENTION_DAYS).unwrap();
        let cutoff_epoch = chrono::DateTime::parse_from_rfc3339(&cutoff)
            .unwrap()
            .timestamp();
        conn.execute(
            "INSERT INTO telemetry_turns
                (event_key, session_id, provider, ended_at, source_key, source_path)
             VALUES ('old', 'session-a', 'codex', ?1, 'source-a', 'log')",
            [&old],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO telemetry_limits(provider, limit_id, observed_at)
             VALUES ('codex', 'limit-a', '2026-01-01T00:00:00.000Z'),
                    ('codex', 'limit-a', '2026-01-01T00:01:00.000Z')",
            [],
        )
        .unwrap();
        std::fs::create_dir_all(&backup_directory).unwrap();
        conn.execute(
            "VACUUM INTO ?1",
            rusqlite::params![baseline.to_string_lossy().as_ref()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO telemetry_meta(key, value) VALUES ('telemetry_maintenance_prepared_cutoff', ?1),
                ('telemetry_maintenance_prepared_retain_days', ?2)",
            rusqlite::params![cutoff_epoch, TELEMETRY_RAW_RETENTION_DAYS],
        )
        .unwrap();
        conn.execute_batch(
            "CREATE TRIGGER reject_limit_cleanup
             BEFORE DELETE ON telemetry_limits
             BEGIN
                 SELECT RAISE(ABORT, 'injected post-association failure');
             END;",
        )
        .unwrap();

        assert!(maintain_if_due_at(&conn, directory.path()).is_err());
        let pending = pending_backup_path(&backup_directory);
        let pending_size = std::fs::metadata(&pending).unwrap().len();
        assert!(!baseline.exists());
        assert_eq!(
            conn.query_row::<i64, _, _>(
                "SELECT count(*) FROM telemetry_turns WHERE event_key = 'old'",
                [],
                |row| row.get(0),
            )
            .unwrap(),
            0
        );

        assert!(maintain_if_due_at(&conn, directory.path()).is_err());
        assert_eq!(std::fs::metadata(&pending).unwrap().len(), pending_size);
        assert_eq!(std::fs::read_dir(&backup_directory).unwrap().count(), 1);

        conn.execute_batch("DROP TRIGGER reject_limit_cleanup;")
            .unwrap();
        let report = maintain_if_due_at(&conn, directory.path())
            .unwrap()
            .expect("the prepared retention should complete after the injected failure is removed");
        assert_eq!(report.limits_deleted, 1);
        assert!(!pending.exists());
        assert_eq!(std::fs::read_dir(&backup_directory).unwrap().count(), 1);
        let recovered = std::fs::read_dir(&backup_directory)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .find(|path| is_automatic_backup(path))
            .expect("the pinned legacy baseline should be promoted after recovery");
        let recovered_conn = rusqlite::Connection::open(recovered).unwrap();
        assert_eq!(
            recovered_conn
                .query_row::<i64, _, _>(
                    "SELECT count(*) FROM telemetry_turns WHERE event_key = 'old'",
                    [],
                    |row| row.get(0),
                )
                .unwrap(),
            1
        );
    }

    #[test]
    fn interrupted_legacy_adoption_reuses_pending_baseline() {
        let directory = tempdir().unwrap();
        let database = directory.path().join("state.db");
        let backup_directory = telemetry_backup_directory(directory.path());
        let baseline = backup_directory.join("state.db.telemetry-100-1.backup");
        let conn = rusqlite::Connection::open(&database).unwrap();
        wardian_core::telemetry::run_telemetry_migrations(&conn).unwrap();
        let now = Utc::now();
        let old = (now - Duration::days(TELEMETRY_RAW_RETENTION_DAYS as i64 + 1)).to_rfc3339();
        let cutoff =
            wardian_core::telemetry::retention_cutoff(now, TELEMETRY_RAW_RETENTION_DAYS).unwrap();
        let cutoff_epoch = chrono::DateTime::parse_from_rfc3339(&cutoff)
            .unwrap()
            .timestamp();
        conn.execute(
            "INSERT INTO telemetry_turns
                (event_key, session_id, provider, ended_at, source_key, source_path)
             VALUES ('old', 'session-a', 'codex', ?1, 'source-a', 'log')",
            [&old],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO telemetry_limits(provider, limit_id, observed_at)
             VALUES ('codex', 'limit-a', '2026-01-01T00:00:00.000Z'),
                    ('codex', 'limit-a', '2026-01-01T00:01:00.000Z')",
            [],
        )
        .unwrap();
        std::fs::create_dir_all(&backup_directory).unwrap();
        conn.execute(
            "VACUUM INTO ?1",
            rusqlite::params![baseline.to_string_lossy().as_ref()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO telemetry_meta(key, value) VALUES ('telemetry_maintenance_prepared_cutoff', ?1),
                ('telemetry_maintenance_prepared_retain_days', ?2)",
            rusqlite::params![cutoff_epoch, TELEMETRY_RAW_RETENTION_DAYS],
        )
        .unwrap();

        // Model termination immediately after adopt_legacy_baseline renames
        // the verified legacy file and before core can persist association.
        let pending = pending_backup_path(&backup_directory);
        std::fs::rename(&baseline, &pending).unwrap();
        let pending_size = std::fs::metadata(&pending).unwrap().len();
        conn.execute_batch(
            "CREATE TRIGGER reject_limit_cleanup
             BEFORE DELETE ON telemetry_limits
             BEGIN
                 SELECT RAISE(ABORT, 'injected post-association failure');
             END;",
        )
        .unwrap();

        assert!(maintain_if_due_at(&conn, directory.path()).is_err());
        assert_eq!(std::fs::metadata(&pending).unwrap().len(), pending_size);
        assert!(wardian_core::telemetry::retention_backup_is_prepared(&conn).unwrap());
        assert!(!baseline.exists());

        assert!(maintain_if_due_at(&conn, directory.path()).is_err());
        assert_eq!(std::fs::metadata(&pending).unwrap().len(), pending_size);
        assert_eq!(std::fs::read_dir(&backup_directory).unwrap().count(), 1);

        conn.execute_batch("DROP TRIGGER reject_limit_cleanup;")
            .unwrap();
        maintain_if_due_at(&conn, directory.path())
            .unwrap()
            .expect("the interrupted legacy retention should complete after retry");
        assert!(!pending.exists());
        assert_eq!(std::fs::read_dir(&backup_directory).unwrap().count(), 1);
    }

    #[test]
    fn legacy_adoption_selects_preparation_baseline_over_newer_retry_snapshot() {
        let directory = tempdir().unwrap();
        let database = directory.path().join("state.db");
        let backup_directory = telemetry_backup_directory(directory.path());
        let baseline = backup_directory.join("state.db.telemetry-100-1.backup");
        let retry = backup_directory.join("state.db.telemetry-200-1.backup");
        let conn = rusqlite::Connection::open(&database).unwrap();
        wardian_core::telemetry::run_telemetry_migrations(&conn).unwrap();
        let now = Utc::now();
        let old = (now - Duration::days(TELEMETRY_RAW_RETENTION_DAYS as i64 + 1)).to_rfc3339();
        let cutoff =
            wardian_core::telemetry::retention_cutoff(now, TELEMETRY_RAW_RETENTION_DAYS).unwrap();
        let cutoff_epoch = chrono::DateTime::parse_from_rfc3339(&cutoff)
            .unwrap()
            .timestamp();
        conn.execute(
            "INSERT INTO telemetry_turns
                (event_key, session_id, provider, ended_at, source_key, source_path)
             VALUES ('old', 'session-a', 'codex', ?1, 'source-a', 'log')",
            [&old],
        )
        .unwrap();
        std::fs::create_dir_all(&backup_directory).unwrap();
        conn.execute(
            "VACUUM INTO ?1",
            rusqlite::params![baseline.to_string_lossy().as_ref()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO telemetry_meta(key, value) VALUES ('telemetry_maintenance_prepared_cutoff', ?1),
                ('telemetry_maintenance_prepared_retain_days', ?2)",
            rusqlite::params![cutoff_epoch, TELEMETRY_RAW_RETENTION_DAYS],
        )
        .unwrap();
        conn.execute(
            "DELETE FROM telemetry_turn_facts WHERE event_key = 'old'",
            [],
        )
        .unwrap();
        conn.execute(
            "VACUUM INTO ?1",
            rusqlite::params![retry.to_string_lossy().as_ref()],
        )
        .unwrap();

        let report = maintain_if_due_at(&conn, directory.path())
            .unwrap()
            .expect("legacy prepared retention should resume");

        assert_eq!(report.turns_deleted, 0);
        assert!(!baseline.exists());
        assert!(retry.exists());
        let recovered = std::fs::read_dir(&backup_directory)
            .unwrap()
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| is_automatic_backup(path))
            .find(|path| {
                rusqlite::Connection::open(path)
                    .unwrap()
                    .query_row::<i64, _, _>(
                        "SELECT count(*) FROM telemetry_turns WHERE event_key = 'old'",
                        [],
                        |row| row.get(0),
                    )
                    .unwrap()
                    == 1
            })
            .expect("the pre-preparation baseline must remain recoverable");
        assert_ne!(recovered, retry);
        assert_eq!(std::fs::read_dir(&backup_directory).unwrap().count(), 2);
    }

    #[test]
    fn due_check_resumes_prepared_retention_without_expired_rows() {
        let directory = tempdir().unwrap();
        let database = directory.path().join("state.db");
        let backup_directory = telemetry_backup_directory(directory.path());
        let baseline = backup_directory.join("state.db.telemetry-100-1.backup");
        let conn = rusqlite::Connection::open(&database).unwrap();
        wardian_core::telemetry::run_telemetry_migrations(&conn).unwrap();
        let now = Utc::now();
        let cutoff =
            wardian_core::telemetry::retention_cutoff(now, TELEMETRY_RAW_RETENTION_DAYS).unwrap();
        let cutoff_epoch = chrono::DateTime::parse_from_rfc3339(&cutoff)
            .unwrap()
            .timestamp();
        std::fs::create_dir_all(&backup_directory).unwrap();
        conn.execute(
            "VACUUM INTO ?1",
            rusqlite::params![baseline.to_string_lossy().as_ref()],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO telemetry_meta(key, value) VALUES ('telemetry_maintenance_prepared_cutoff', ?1),
                ('telemetry_maintenance_prepared_retain_days', ?2)",
            rusqlite::params![cutoff_epoch, TELEMETRY_RAW_RETENTION_DAYS],
        )
        .unwrap();

        let report = maintain_if_due_at(&conn, directory.path())
            .unwrap()
            .expect("prepared retention must resume even after its final deletion");

        assert_eq!(report.turns_deleted, 0);
        assert!(!wardian_core::telemetry::retention_is_prepared(&conn).unwrap());
        assert!(!wardian_core::telemetry::retention_backup_is_prepared(&conn).unwrap());
        assert!(!baseline.exists());
        assert!(!pending_backup_path(&backup_directory).exists());
        assert_eq!(std::fs::read_dir(&backup_directory).unwrap().count(), 1);
    }

    #[test]
    fn failed_preparation_reuses_one_pending_backup_across_retries() {
        let directory = tempdir().unwrap();
        let database = directory.path().join("state.db");
        let backup_directory = telemetry_backup_directory(directory.path());
        let pending = pending_backup_path(&backup_directory);
        let conn = rusqlite::Connection::open(&database).unwrap();
        wardian_core::telemetry::run_telemetry_migrations(&conn).unwrap();
        let old =
            (Utc::now() - Duration::days(TELEMETRY_RAW_RETENTION_DAYS as i64 + 1)).to_rfc3339();
        conn.execute(
            "INSERT INTO telemetry_turns
                (event_key, session_id, provider, ended_at, source_key, source_path)
             VALUES ('old', 'session-a', 'codex', ?1, 'source-a', 'log')",
            [&old],
        )
        .unwrap();
        conn.execute_batch(
            "CREATE TRIGGER reject_rollup_prepare
             BEFORE INSERT ON telemetry_rollup_hourly
             BEGIN
                 SELECT RAISE(ABORT, 'injected preparation failure');
             END;",
        )
        .unwrap();

        assert!(maintain_if_due_at(&conn, directory.path()).is_err());
        assert!(maintain_if_due_at(&conn, directory.path()).is_err());

        assert!(pending.exists());
        assert!(wardian_core::telemetry::retention_backup_is_prepared(&conn).unwrap());
        assert!(!PathBuf::from(format!(
            "{}{}",
            pending.to_string_lossy(),
            AUTOMATIC_BACKUP_TEMP_SUFFIX
        ))
        .exists());
        let files = std::fs::read_dir(&backup_directory)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
            .count();
        assert_eq!(files, 1);
    }
}
