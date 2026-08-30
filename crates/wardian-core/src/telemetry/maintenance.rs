//! Explicit, operator-driven telemetry retention and compaction.
//!
//! Raw turns, edits, and completed activity intervals are safe to remove only
//! after their hourly buckets have been recomputed. Rate-limit observations do
//! not have an equivalent rollup, so this path deliberately retains them.
//!
//! The caller supplies the retention window and a new backup destination. No
//! age policy is silently chosen, and compaction is opt-in because `VACUUM`
//! rewrites the whole database.

use crate::telemetry::rollup::recompute_buckets;
use crate::telemetry::schema::{acquire_telemetry_lock, sqlite_io_error};
use crate::telemetry::store::{mark_dirty, mark_dirty_span, DirtyBuckets};
use chrono::{DateTime, Duration, Timelike, Utc};
use rusqlite::{params, Connection, OpenFlags};
use std::io;
use std::path::Path;

const DELETE_BATCH_SIZE: i64 = 2_000;

/// The durable effects of one explicit telemetry maintenance run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaintenanceReport {
    pub cutoff: String,
    pub turns_deleted: i64,
    pub edits_deleted: i64,
    pub activity_deleted: i64,
    pub limits_retained: i64,
    pub wal_log_frames: i64,
    pub wal_checkpointed_frames: i64,
    pub vacuumed: bool,
}

/// Retain the requested number of days of raw telemetry and optionally compact
/// the database after creating and verifying a backup.
///
/// This is intentionally an explicit maintenance operation. The caller must
/// stop the desktop app and agents before invoking it; the backup is verified
/// before any source row is deleted, and `vacuum` must be opted into because
/// it rewrites the whole database.
pub fn maintain(
    conn: &Connection,
    retain_days: u32,
    backup_path: &Path,
    vacuum: bool,
) -> rusqlite::Result<MaintenanceReport> {
    maintain_at(conn, retain_days, backup_path, vacuum, Utc::now())
}

fn maintain_at(
    conn: &Connection,
    retain_days: u32,
    backup_path: &Path,
    vacuum: bool,
    now: DateTime<Utc>,
) -> rusqlite::Result<MaintenanceReport> {
    if retain_days == 0 {
        return Err(invalid_request("retention must be at least one day"));
    }
    conn.busy_timeout(std::time::Duration::from_secs(30))?;
    let _telemetry_lock = acquire_telemetry_lock(conn)?;
    create_verified_backup(conn, backup_path)?;

    let cutoff = retention_cutoff(now, retain_days);
    let dirty = dirty_buckets_before(conn, &cutoff)?;
    recompute_buckets(conn, &dirty)?;

    let turns_deleted =
        delete_facts_before(conn, "telemetry_turn_facts", "ended_at", &cutoff, "<")?;
    let edits_deleted =
        delete_facts_before(conn, "telemetry_edit_facts", "occurred_at", &cutoff, "<")?;
    let activity_deleted =
        delete_facts_before(conn, "telemetry_activity", "ended_at", &cutoff, "<=")?;
    let limits_retained = conn.query_row("SELECT count(*) FROM telemetry_limits", [], |row| {
        row.get(0)
    })?;

    let mut checkpoint = checkpoint_wal(conn)?;
    if vacuum {
        conn.execute_batch("VACUUM")?;
        checkpoint = checkpoint_wal(conn)?;
    }

    Ok(MaintenanceReport {
        cutoff,
        turns_deleted,
        edits_deleted,
        activity_deleted,
        limits_retained,
        wal_log_frames: checkpoint.1,
        wal_checkpointed_frames: checkpoint.2,
        vacuumed: vacuum,
    })
}

/// Round the requested retention boundary down to an hour so only complete
/// rollup buckets are retired.
pub fn retention_cutoff(now: DateTime<Utc>, retain_days: u32) -> String {
    let raw = now - Duration::days(i64::from(retain_days));
    let floored = raw
        .with_minute(0)
        .and_then(|value| value.with_second(0))
        .and_then(|value| value.with_nanosecond(0))
        .unwrap_or(raw);
    floored.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn create_verified_backup(conn: &Connection, backup_path: &Path) -> rusqlite::Result<()> {
    if backup_path.exists() {
        return Err(invalid_request(format!(
            "backup destination already exists: {}",
            backup_path.display()
        )));
    }
    let backup_path_string = backup_path.to_string_lossy().into_owned();
    conn.execute("VACUUM INTO ?1", params![backup_path_string])?;

    let backup = Connection::open_with_flags(backup_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let integrity: String = backup.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if integrity != "ok" {
        return Err(invalid_request(format!(
            "backup integrity check failed: {integrity}"
        )));
    }
    Ok(())
}

fn dirty_buckets_before(conn: &Connection, cutoff: &str) -> rusqlite::Result<DirtyBuckets> {
    let mut dirty = DirtyBuckets::new();
    for (table, timestamp_column) in [
        ("telemetry_turns", "ended_at"),
        ("telemetry_edits", "occurred_at"),
    ] {
        let mut statement = conn.prepare(&format!(
            "SELECT session_id, {timestamp_column}
             FROM {table} WHERE {timestamp_column} < ?1"
        ))?;
        let rows = statement.query_map(params![cutoff], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (session_id, timestamp) = row?;
            mark_dirty(&mut dirty, &session_id, &timestamp);
        }
    }

    let mut statement = conn.prepare(
        "SELECT session_id, started_at, ended_at
         FROM telemetry_activity WHERE ended_at <= ?1",
    )?;
    let rows = statement.query_map(params![cutoff], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;
    for row in rows {
        let (session_id, started_at, ended_at) = row?;
        mark_dirty_span(&mut dirty, &session_id, &started_at, &ended_at);
    }
    Ok(dirty)
}

fn delete_facts_before(
    conn: &Connection,
    table: &str,
    timestamp_column: &str,
    cutoff: &str,
    operator: &str,
) -> rusqlite::Result<i64> {
    let mut total = 0;
    loop {
        let deleted = conn.execute(
            &format!(
                "DELETE FROM {table}
                 WHERE id IN (
                     SELECT id FROM {table}
                     WHERE {timestamp_column} {operator} ?1
                     LIMIT ?2
                 )"
            ),
            params![cutoff, DELETE_BATCH_SIZE],
        )?;
        total += i64::try_from(deleted).unwrap_or(i64::MAX);
        if deleted == 0 {
            return Ok(total);
        }
    }
}

fn checkpoint_wal(conn: &Connection) -> rusqlite::Result<(i64, i64, i64)> {
    let result = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    })?;
    if result.0 != 0 {
        return Err(invalid_request(format!(
            "WAL checkpoint was busy ({})",
            result.0
        )));
    }
    Ok(result)
}

fn invalid_request(message: impl Into<String>) -> rusqlite::Error {
    sqlite_io_error(io::Error::new(io::ErrorKind::InvalidInput, message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::schema::run_telemetry_migrations;
    use tempfile::tempdir;

    #[test]
    fn retention_cutoff_is_hour_aligned() {
        let now = DateTime::parse_from_rfc3339("2026-08-30T12:37:45.123Z")
            .unwrap()
            .with_timezone(&Utc);
        assert_eq!(retention_cutoff(now, 90), "2026-06-01T12:00:00.000Z");
    }

    #[test]
    fn maintenance_requires_explicit_retention() {
        let conn = Connection::open_in_memory().unwrap();
        run_telemetry_migrations(&conn).unwrap();
        let error = maintain(&conn, 0, Path::new("backup.db"), false).unwrap_err();
        assert!(error.to_string().contains("at least one day"));
    }

    #[test]
    fn maintenance_recomputes_rollups_before_deleting_old_facts() {
        let directory = tempdir().unwrap();
        let database = directory.path().join("state.db");
        let backup = directory.path().join("state.db.backup");
        let conn = Connection::open(&database).unwrap();
        run_telemetry_migrations(&conn).unwrap();
        conn.execute(
            "INSERT INTO telemetry_turns
                (event_key, session_id, provider, turn_id, model, ended_at,
                 input_tokens, output_tokens, source_key, source_path)
             VALUES ('old-event', 'session-a', 'codex', 'turn-a', 'model-a',
                     '2026-01-01T00:15:00.000Z', 11, 7, 'source-a', 'log')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO telemetry_edits
                (event_key, session_id, provider, turn_id, occurred_at, path, op,
                 lines_added, lines_removed, source_key, source_path)
             VALUES ('old-edit', 'session-a', 'codex', 'turn-a',
                     '2026-01-01T00:20:00.000Z', 'src/lib.rs', 'modify', 4, 1,
                     'source-a', 'log')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO telemetry_activity
                (session_id, provider, started_at, ended_at, last_event_at,
                 event_count, method, source_key)
             VALUES ('session-a', 'codex', '2026-01-01T00:10:00.000Z',
                     '2026-01-01T00:30:00.000Z', '2026-01-01T00:30:00.000Z',
                     1, 'measured', 'source-a')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO telemetry_limits(provider, limit_id, observed_at)
             VALUES ('codex', 'limit-a', '2026-01-01T00:00:00.000Z')",
            [],
        )
        .unwrap();

        let now = DateTime::parse_from_rfc3339("2026-08-30T12:00:00.000Z")
            .unwrap()
            .with_timezone(&Utc);
        let report = maintain_at(&conn, 1, &backup, false, now).unwrap();
        assert!(report.turns_deleted >= 1);
        assert_eq!(report.edits_deleted, 1);
        assert_eq!(report.activity_deleted, 1);
        assert_eq!(report.limits_retained, 1);
        assert_eq!(
            conn.query_row::<i64, _, _>(
                "SELECT turns FROM telemetry_rollup_hourly
                 WHERE bucket_start = '2026-01-01T00:00:00.000Z'",
                [],
                |row| row.get(0),
            )
            .unwrap(),
            1
        );
        assert!(backup.exists());
    }
}
