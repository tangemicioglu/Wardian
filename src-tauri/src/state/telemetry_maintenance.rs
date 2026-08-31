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

fn is_automatic_backup(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            name.starts_with(AUTOMATIC_BACKUP_PREFIX) && name.ends_with(AUTOMATIC_BACKUP_SUFFIX)
        })
}

fn prune_automatic_backups(directory: &Path) -> io::Result<()> {
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

    let remove_count = backups.len().saturating_sub(AUTOMATIC_BACKUP_COUNT);
    for backup in backups.into_iter().take(remove_count) {
        std::fs::remove_file(backup)?;
    }
    Ok(())
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
    if !wardian_core::telemetry::has_expired_raw_telemetry(conn, TELEMETRY_RAW_RETENTION_DAYS)? {
        return Ok(None);
    }

    std::fs::create_dir_all(&backup_directory)?;
    let backup_path = new_backup_path(&backup_directory);
    let report =
        wardian_core::telemetry::maintain(conn, TELEMETRY_RAW_RETENTION_DAYS, &backup_path, true)?;

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
}
