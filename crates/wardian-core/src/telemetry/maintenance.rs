//! Core-owned telemetry retention and compaction.
//!
//! Raw turns, edits, and completed activity intervals are safe to remove only
//! after their hourly buckets have been recomputed. The ingest path keeps one
//! current rate-limit gauge per provider; maintenance can clean legacy rows
//! from installed databases.
//!
//! The application maintenance service supplies the retention window and a new
//! backup destination. No age policy is silently chosen, and compaction is
//! opt-in because `VACUUM` rewrites the whole database.

use crate::telemetry::rollup::recompute_buckets;
use crate::telemetry::schema::{acquire_telemetry_lock, sqlite_io_error};
use crate::telemetry::store::{mark_dirty, mark_dirty_span, DirtyBuckets};
use chrono::{DateTime, Duration, Timelike, Utc};
use rusqlite::{
    params, Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior,
};
use std::io;
use std::path::Path;

const DELETE_BATCH_SIZE: i64 = 2_000;
/// Upper bound accepted by retention so subtracting the window stays within
/// chrono's representable `DateTime<Utc>` range for every current timestamp.
pub const MAX_RETENTION_DAYS: u32 = 90_000_000;
const RETENTION_PREPARED_KEY: &str = "telemetry_maintenance_prepared_cutoff";
const RETENTION_WINDOW_KEY: &str = "telemetry_maintenance_prepared_retain_days";
const RETENTION_BACKUP_PENDING_KEY: &str = "telemetry_maintenance_backup_pending";
const RETENTION_BACKUP_KEY: &str = "telemetry_maintenance_backup_prepared";
const BACKUP_TEMP_SUFFIX: &str = ".tmp";

/// The durable effects of one explicit telemetry maintenance run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaintenanceReport {
    pub cutoff: String,
    pub turns_deleted: i64,
    pub edits_deleted: i64,
    pub activity_deleted: i64,
    pub limits_deleted: i64,
    pub limits_retained: i64,
    pub wal_log_frames: i64,
    pub wal_checkpointed_frames: i64,
    pub vacuumed: bool,
}

/// Retain the requested number of days of raw telemetry and optionally compact
/// the database after creating and verifying a backup.
///
/// The application maintenance service calls this through its database
/// serialization boundary. The backup is verified before any source row is
/// deleted; callers resuming a prepared retention phase may pass its existing
/// verified baseline. `vacuum` must be opted into because it rewrites the
/// whole database.
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
    let mut no_hook: Option<&mut dyn FnMut() -> rusqlite::Result<()>> = None;
    maintain_at_with_hook(conn, retain_days, backup_path, vacuum, now, &mut no_hook)
}

fn maintain_at_with_hook(
    conn: &Connection,
    retain_days: u32,
    backup_path: &Path,
    vacuum: bool,
    now: DateTime<Utc>,
    after_batch: &mut Option<&mut dyn FnMut() -> rusqlite::Result<()>>,
) -> rusqlite::Result<MaintenanceReport> {
    if retain_days == 0 {
        return Err(invalid_request("retention must be at least one day"));
    }
    let requested_cutoff = retention_cutoff(now, retain_days)?;
    conn.busy_timeout(std::time::Duration::from_secs(30))?;
    let _telemetry_lock = acquire_telemetry_lock(conn)?;
    mark_backup_pending(conn)?;
    create_verified_backup(conn, backup_path)?;
    mark_backup_prepared(conn)?;

    let cutoff = prepare_retention_rollups(conn, &requested_cutoff, retain_days)?;
    let turns_deleted = delete_turns_before(conn, &cutoff, after_batch)?;
    let edits_deleted = delete_edits_before(conn, &cutoff, after_batch)?;
    let activity_deleted = delete_activity_before(conn, &cutoff, after_batch)?;
    let limits_deleted = delete_stale_limits(conn, after_batch)?;
    let limits_retained = conn.query_row("SELECT count(*) FROM telemetry_limits", [], |row| {
        row.get(0)
    })?;

    let mut checkpoint = checkpoint_wal(conn)?;
    if vacuum {
        conn.execute_batch("VACUUM")?;
        checkpoint = checkpoint_wal(conn)?;
    }
    clear_retention_marker(conn, &cutoff)?;

    Ok(MaintenanceReport {
        cutoff,
        turns_deleted,
        edits_deleted,
        activity_deleted,
        limits_deleted,
        limits_retained,
        wal_log_frames: checkpoint.1,
        wal_checkpointed_frames: checkpoint.2,
        vacuumed: vacuum,
    })
}

/// Round the requested retention boundary down to an hour so only complete
/// rollup buckets are retired.
pub fn retention_cutoff(now: DateTime<Utc>, retain_days: u32) -> rusqlite::Result<String> {
    if retain_days == 0 || retain_days > MAX_RETENTION_DAYS {
        return Err(invalid_request(
            "retention window is outside the supported date range",
        ));
    }
    let raw = now
        .checked_sub_signed(Duration::days(i64::from(retain_days)))
        .ok_or_else(|| invalid_request("retention window is outside the supported date range"))?;
    let floored = raw
        .with_minute(0)
        .and_then(|value| value.with_second(0))
        .and_then(|value| value.with_nanosecond(0))
        .unwrap_or(raw);
    Ok(floored.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

/// Return whether any raw telemetry covered by the retention policy is ready
/// for maintenance.
///
/// The application checks this before creating a full backup. A scheduled
/// maintenance pass that has nothing to prune must be cheap and must not create
/// another copy of the database merely because the app restarted.
pub fn has_expired_raw_telemetry(conn: &Connection, retain_days: u32) -> rusqlite::Result<bool> {
    has_expired_raw_telemetry_at(conn, retain_days, Utc::now())
}

fn has_expired_raw_telemetry_at(
    conn: &Connection,
    retain_days: u32,
    now: DateTime<Utc>,
) -> rusqlite::Result<bool> {
    let cutoff = retention_cutoff(now, retain_days)?;
    conn.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM telemetry_turns
             WHERE julianday(ended_at) < julianday(?1)
         ) OR EXISTS(
             SELECT 1 FROM telemetry_edits
             WHERE julianday(occurred_at) < julianday(?1)
        ) OR EXISTS(
             SELECT 1 FROM telemetry_activity
             WHERE julianday(ended_at) <= julianday(?1)
         ) OR EXISTS(
             SELECT 1 FROM telemetry_limits older
             WHERE EXISTS(
                 SELECT 1 FROM telemetry_limits newer
                 WHERE newer.provider = older.provider
                   AND (newer.observed_at > older.observed_at
                        OR (newer.observed_at = older.observed_at AND newer.id > older.id))
             )
         )",
        params![cutoff],
        |row| row.get::<_, i64>(0),
    )
    .map(|value| value != 0)
}

fn create_verified_backup(conn: &Connection, backup_path: &Path) -> rusqlite::Result<()> {
    if backup_path.exists() {
        return verify_backup(backup_path);
    }

    let temporary_path = backup_temp_path(backup_path);
    if temporary_path.exists() {
        std::fs::remove_file(&temporary_path).map_err(sqlite_io_error)?;
    }

    let result = (|| {
        let temporary_path_string = temporary_path.to_string_lossy().into_owned();
        conn.execute("VACUUM INTO ?1", params![temporary_path_string])?;
        verify_backup(&temporary_path)?;
        std::fs::rename(&temporary_path, backup_path).map_err(sqlite_io_error)?;
        Ok(())
    })();

    if result.is_err() {
        let _ = std::fs::remove_file(&temporary_path);
    }
    result
}

fn backup_temp_path(backup_path: &Path) -> std::path::PathBuf {
    std::path::PathBuf::from(format!(
        "{}{}",
        backup_path.to_string_lossy(),
        BACKUP_TEMP_SUFFIX
    ))
}

/// Verify that a telemetry backup is readable and passes SQLite integrity
/// checks. Existing verified backups may be supplied to [`maintain`] when an
/// interrupted retention run is being resumed.
pub fn verify_backup(backup_path: &Path) -> rusqlite::Result<()> {
    let backup = Connection::open_with_flags(backup_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let integrity: String = backup.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if integrity != "ok" {
        return Err(invalid_request(format!(
            "backup integrity check failed: {integrity}"
        )));
    }
    Ok(())
}

/// Return whether a destructive retention phase has already prepared its
/// cutoff and must resume with the same retention window.
pub fn retention_is_prepared(conn: &Connection) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM telemetry_meta WHERE key = ?1
         )",
        params![RETENTION_PREPARED_KEY],
        |row| row.get::<_, i64>(0),
    )
    .map(|value| value != 0)
}

/// Return whether a verified backup has been durably associated with the
/// current retention attempt. The application uses this to distinguish a
/// retry baseline from a pending file left after a completed run.
pub fn retention_backup_is_prepared(conn: &Connection) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM telemetry_meta
             WHERE key = ?1 AND value = 1
         )",
        params![RETENTION_BACKUP_KEY],
        |row| row.get::<_, i64>(0),
    )
    .map(|value| value != 0)
}

/// Return whether a retention attempt has started backup work but has not yet
/// durably associated its verified baseline. This distinguishes a pending file
/// left by an interrupted attempt from one left after a completed run.
pub fn retention_backup_is_pending(conn: &Connection) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM telemetry_meta
             WHERE key = ?1 AND value = 1
         )",
        params![RETENTION_BACKUP_PENDING_KEY],
        |row| row.get::<_, i64>(0),
    )
    .map(|value| value != 0)
}

fn mark_backup_pending(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO telemetry_meta (key, value) VALUES (?1, 1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![RETENTION_BACKUP_PENDING_KEY],
    )?;
    Ok(())
}

fn mark_backup_prepared(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO telemetry_meta (key, value) VALUES (?1, 1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![RETENTION_BACKUP_KEY],
    )?;
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
             FROM {table}
             WHERE julianday({timestamp_column}) < julianday(?1)"
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
         FROM telemetry_activity
         WHERE julianday(ended_at) <= julianday(?1)",
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

fn cutoff_epoch(cutoff: &str) -> rusqlite::Result<i64> {
    DateTime::parse_from_rfc3339(cutoff)
        .map(|value| value.timestamp())
        .map_err(|error| invalid_request(format!("invalid retention cutoff: {error}")))
}

fn cutoff_from_epoch(epoch: i64) -> rusqlite::Result<String> {
    DateTime::<Utc>::from_timestamp(epoch, 0)
        .map(|value| value.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .ok_or_else(|| invalid_request(format!("invalid persisted retention cutoff: {epoch}")))
}

fn prepare_retention_rollups(
    conn: &Connection,
    requested_cutoff: &str,
    retain_days: u32,
) -> rusqlite::Result<String> {
    let requested_epoch = cutoff_epoch(requested_cutoff)?;
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    let prepared = tx
        .query_row(
            "SELECT value FROM telemetry_meta WHERE key = ?1",
            params![RETENTION_PREPARED_KEY],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    if let Some(previous) = prepared {
        let previous_retain_days = tx
            .query_row(
                "SELECT value FROM telemetry_meta WHERE key = ?1",
                params![RETENTION_WINDOW_KEY],
                |row| row.get::<_, u32>(0),
            )
            .optional()?;
        if let Some(previous_retain_days) = previous_retain_days {
            if previous_retain_days != retain_days {
                return Err(invalid_request(
                    "an interrupted retention run exists; resume it with the same retention window",
                ));
            }
        } else {
            // Older prepared markers did not persist the requested window.
            // Adopt the current request only as metadata; the already
            // prepared cutoff remains the canonical recovery boundary.
            tx.execute(
                "INSERT INTO telemetry_meta (key, value) VALUES (?1, ?2)",
                params![RETENTION_WINDOW_KEY, retain_days],
            )?;
            tx.commit()?;
        }
        return cutoff_from_epoch(previous);
    }

    let dirty = dirty_buckets_before(&tx, requested_cutoff)?;
    recompute_buckets(&tx, &dirty)?;
    tx.execute(
        "INSERT INTO telemetry_meta (key, value) VALUES (?1, ?2)",
        params![RETENTION_PREPARED_KEY, requested_epoch],
    )?;
    tx.execute(
        "INSERT INTO telemetry_meta (key, value) VALUES (?1, ?2)",
        params![RETENTION_WINDOW_KEY, retain_days],
    )?;
    tx.commit()?;
    Ok(requested_cutoff.to_owned())
}

fn clear_retention_marker(conn: &Connection, cutoff: &str) -> rusqlite::Result<()> {
    let cutoff_epoch = cutoff_epoch(cutoff)?;
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    tx.execute(
        "DELETE FROM telemetry_meta WHERE key = ?1 AND value = ?2",
        params![RETENTION_PREPARED_KEY, cutoff_epoch],
    )?;
    tx.execute(
        "DELETE FROM telemetry_meta WHERE key = ?1",
        params![RETENTION_WINDOW_KEY],
    )?;
    tx.execute(
        "DELETE FROM telemetry_meta WHERE key = ?1 AND value = 1",
        params![RETENTION_BACKUP_KEY],
    )?;
    tx.execute(
        "DELETE FROM telemetry_meta WHERE key = ?1 AND value = 1",
        params![RETENTION_BACKUP_PENDING_KEY],
    )?;
    tx.commit()
}

fn delete_turns_before(
    conn: &Connection,
    cutoff: &str,
    after_batch: &mut Option<&mut dyn FnMut() -> rusqlite::Result<()>>,
) -> rusqlite::Result<i64> {
    let mut total = 0;
    loop {
        let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
        let rows = {
            let mut statement = tx.prepare(
                "SELECT id, session_id, ended_at
                 FROM telemetry_turns
                 WHERE julianday(ended_at) < julianday(?1)
                 ORDER BY id
                 LIMIT ?2",
            )?;
            let rows = statement
                .query_map(params![cutoff, DELETE_BATCH_SIZE], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        if rows.is_empty() {
            tx.rollback()?;
            return Ok(total);
        }
        for (id, _session_id, _ended_at) in &rows {
            tx.execute(
                "DELETE FROM telemetry_turn_facts WHERE id = ?1",
                params![id],
            )?;
        }
        tx.commit()?;
        total += i64::try_from(rows.len()).unwrap_or(i64::MAX);
        run_batch_hook(after_batch)?;
    }
}

fn delete_edits_before(
    conn: &Connection,
    cutoff: &str,
    after_batch: &mut Option<&mut dyn FnMut() -> rusqlite::Result<()>>,
) -> rusqlite::Result<i64> {
    let mut total = 0;
    loop {
        let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
        let rows = {
            let mut statement = tx.prepare(
                "SELECT id, session_id, occurred_at
                 FROM telemetry_edits
                 WHERE julianday(occurred_at) < julianday(?1)
                 ORDER BY id
                 LIMIT ?2",
            )?;
            let rows = statement
                .query_map(params![cutoff, DELETE_BATCH_SIZE], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        if rows.is_empty() {
            tx.rollback()?;
            return Ok(total);
        }
        for (id, _session_id, _occurred_at) in &rows {
            tx.execute(
                "DELETE FROM telemetry_edit_facts WHERE id = ?1",
                params![id],
            )?;
        }
        tx.commit()?;
        total += i64::try_from(rows.len()).unwrap_or(i64::MAX);
        run_batch_hook(after_batch)?;
    }
}

fn delete_activity_before(
    conn: &Connection,
    cutoff: &str,
    after_batch: &mut Option<&mut dyn FnMut() -> rusqlite::Result<()>>,
) -> rusqlite::Result<i64> {
    let mut total = 0;
    loop {
        let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
        let rows = {
            let mut statement = tx.prepare(
                "SELECT id, session_id, started_at, ended_at
                 FROM telemetry_activity
                 WHERE julianday(ended_at) <= julianday(?1)
                 ORDER BY id
                 LIMIT ?2",
            )?;
            let rows = statement
                .query_map(params![cutoff, DELETE_BATCH_SIZE], |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        if rows.is_empty() {
            tx.rollback()?;
            return Ok(total);
        }
        for (id, _session_id, _started_at, _ended_at) in &rows {
            tx.execute("DELETE FROM telemetry_activity WHERE id = ?1", params![id])?;
        }
        tx.commit()?;
        total += i64::try_from(rows.len()).unwrap_or(i64::MAX);
        run_batch_hook(after_batch)?;
    }
}

/// Retain the latest account-level limit gauge for each provider.
///
/// Limit observations arrive with the same heartbeat as token counts, but no
/// current interface asks for their history. Deleting older observations here
/// keeps an existing database bounded and the write path also avoids creating
/// new history after this cleanup runs.
fn delete_stale_limits(
    conn: &Connection,
    after_batch: &mut Option<&mut dyn FnMut() -> rusqlite::Result<()>>,
) -> rusqlite::Result<i64> {
    let mut total = 0;
    loop {
        let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
        let ids = {
            let mut statement = tx.prepare(
                "SELECT older.id FROM telemetry_limits older
                 WHERE EXISTS(
                     SELECT 1 FROM telemetry_limits newer
                     WHERE newer.provider = older.provider
                       AND (newer.observed_at > older.observed_at
                            OR (newer.observed_at = older.observed_at AND newer.id > older.id))
                 )
                 ORDER BY older.id
                 LIMIT ?1",
            )?;
            let ids = statement
                .query_map(params![DELETE_BATCH_SIZE], |row| row.get::<_, i64>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            ids
        };
        if ids.is_empty() {
            tx.rollback()?;
            return Ok(total);
        }
        for id in &ids {
            tx.execute("DELETE FROM telemetry_limits WHERE id = ?1", params![id])?;
        }
        tx.commit()?;
        total += i64::try_from(ids.len()).unwrap_or(i64::MAX);
        run_batch_hook(after_batch)?;
    }
}

fn run_batch_hook(
    after_batch: &mut Option<&mut dyn FnMut() -> rusqlite::Result<()>>,
) -> rusqlite::Result<()> {
    if let Some(hook) = after_batch.as_deref_mut() {
        hook()?;
    }
    Ok(())
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
        assert_eq!(
            retention_cutoff(now, 90).unwrap(),
            "2026-06-01T12:00:00.000Z"
        );
    }

    #[test]
    fn expired_check_only_reports_raw_facts_outside_the_window() {
        let conn = Connection::open_in_memory().unwrap();
        run_telemetry_migrations(&conn).unwrap();
        conn.execute(
            "INSERT INTO telemetry_turns
                (event_key, session_id, provider, ended_at, source_key, source_path)
             VALUES ('recent', 'session-a', 'codex', '2026-08-29T12:01:00.000Z', 'source-a', 'log')",
            [],
        )
        .unwrap();

        let now = DateTime::parse_from_rfc3339("2026-08-30T12:00:00.000Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(!has_expired_raw_telemetry_at(&conn, 1, now).unwrap());

        conn.execute(
            "INSERT INTO telemetry_edits
                (event_key, session_id, provider, occurred_at, path, op, source_key, source_path)
             VALUES ('old', 'session-a', 'codex', '2026-08-29T11:59:59.999Z', 'a.rs', 'modify', 'source-a', 'log')",
            [],
        )
        .unwrap();
        assert!(has_expired_raw_telemetry_at(&conn, 1, now).unwrap());
    }

    #[test]
    fn retention_rejects_windows_outside_the_supported_date_range_before_backup() {
        let directory = tempdir().unwrap();
        let database = directory.path().join("state.db");
        let backup = directory.path().join("state.db.backup");
        let conn = Connection::open(&database).unwrap();
        run_telemetry_migrations(&conn).unwrap();
        let now = DateTime::parse_from_rfc3339("2026-08-30T12:37:45.123Z")
            .unwrap()
            .with_timezone(&Utc);

        let error = maintain_at(&conn, u32::MAX, &backup, false, now).unwrap_err();

        assert!(error.to_string().contains("supported date range"));
        assert!(!backup.exists());
    }

    #[test]
    fn failed_backup_creation_removes_the_temporary_destination() {
        let directory = tempdir().unwrap();
        let database = directory.path().join("state.db");
        let backup = directory
            .path()
            .join("missing-parent")
            .join("state.db.backup");
        let conn = Connection::open(&database).unwrap();
        run_telemetry_migrations(&conn).unwrap();

        assert!(maintain_at(
            &conn,
            1,
            &backup,
            false,
            DateTime::parse_from_rfc3339("2026-08-30T12:00:00.000Z")
                .unwrap()
                .with_timezone(&Utc),
        )
        .is_err());
        assert!(!backup.exists());
        assert!(!backup_temp_path(&backup).exists());
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
        conn.execute(
            "INSERT INTO telemetry_limits(provider, limit_id, observed_at)
             VALUES ('codex', 'limit-a', '2026-01-01T00:01:00.000Z')",
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
        assert_eq!(report.limits_deleted, 1);
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

    #[test]
    fn maintenance_compares_rfc3339_timestamps_chronologically() {
        let directory = tempdir().unwrap();
        let database = directory.path().join("state.db");
        let backup = directory.path().join("state.db.backup");
        let conn = Connection::open(&database).unwrap();
        run_telemetry_migrations(&conn).unwrap();
        for (event_key, ended_at) in [
            ("before-with-offset", "2026-08-29T13:30:00+02:00"),
            ("after-with-offset", "2026-08-29T13:30:00+01:00"),
            ("exact-boundary", "2026-08-29T12:00:00+00:00"),
            ("before-with-fraction", "2026-08-29T11:59:59.999Z"),
            ("after-with-fraction", "2026-08-29T12:00:00.001Z"),
        ] {
            conn.execute(
                "INSERT INTO telemetry_turns
                    (event_key, session_id, provider, turn_id, model, ended_at,
                     input_tokens, output_tokens, source_key, source_path)
                 VALUES (?1, 'session-a', 'codex', ?2, 'model-a', ?3,
                         1, 1, 'source-a', 'log')",
                params![event_key, event_key, ended_at],
            )
            .unwrap();
        }
        for (event_key, occurred_at) in [
            ("edit-before-with-fraction", "2026-08-29T11:59:59.999Z"),
            ("edit-after-with-fraction", "2026-08-29T12:00:00.001Z"),
        ] {
            conn.execute(
                "INSERT INTO telemetry_edits
                    (event_key, session_id, provider, turn_id, occurred_at, path, op,
                     lines_added, lines_removed, source_key, source_path)
                 VALUES (?1, 'session-a', 'codex', ?2, ?3, 'src/lib.rs', 'modify',
                         1, 1, 'source-a', 'log')",
                params![event_key, event_key, occurred_at],
            )
            .unwrap();
        }
        for (method, started_at, ended_at) in [
            (
                "measured",
                "2026-08-29T11:59:00.000Z",
                "2026-08-29T11:59:59.999Z",
            ),
            (
                "measured",
                "2026-08-29T12:00:00.000Z",
                "2026-08-29T12:00:00.500Z",
            ),
        ] {
            conn.execute(
                "INSERT INTO telemetry_activity
                    (session_id, provider, started_at, ended_at, last_event_at,
                     event_count, method, source_key)
                 VALUES ('session-a', 'codex', ?1, ?2, ?2, 1, ?3, 'source-a')",
                params![started_at, ended_at, method],
            )
            .unwrap();
        }

        let now = DateTime::parse_from_rfc3339("2026-08-30T12:00:00.000Z")
            .unwrap()
            .with_timezone(&Utc);
        let report = maintain_at(&conn, 1, &backup, false, now).unwrap();
        assert_eq!(report.turns_deleted, 2);
        assert_eq!(report.edits_deleted, 1);
        assert_eq!(report.activity_deleted, 1);
        assert_eq!(
            conn.query_row::<i64, _, _>("SELECT count(*) FROM telemetry_turns", [], |row| {
                row.get(0)
            })
            .unwrap(),
            3
        );
        assert_eq!(
            conn.query_row::<i64, _, _>(
                "SELECT count(*) FROM telemetry_turns
                 WHERE event_key IN ('after-with-offset', 'exact-boundary',
                                     'after-with-fraction')",
                [],
                |row| row.get(0),
            )
            .unwrap(),
            3
        );
        assert_eq!(
            conn.query_row::<i64, _, _>("SELECT count(*) FROM telemetry_edits", [], |row| {
                row.get(0)
            })
            .unwrap(),
            1
        );
        assert_eq!(
            conn.query_row::<i64, _, _>("SELECT count(*) FROM telemetry_activity", [], |row| {
                row.get(0)
            })
            .unwrap(),
            1
        );
        assert_eq!(
            conn.query_row::<(i64, i64, i64, i64, i64), _, _>(
                "SELECT turns, input_tokens, measured_active_ms, files_touched,
                        lines_added
                 FROM telemetry_rollup_hourly
                 WHERE bucket_start = '2026-08-29T11:00:00.000Z'
                   AND session_id = 'session-a'
                   AND model = 'model-a'",
                [],
                |row| Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?
                )),
            )
            .unwrap(),
            (2, 2, 59_999, 1, 1)
        );
    }

    #[test]
    fn interrupted_retention_rebuilds_each_committed_batch_on_retry() {
        let directory = tempdir().unwrap();
        let database = directory.path().join("state.db");
        let first_backup = directory.path().join("state.db.first.backup");
        let conn = Connection::open(&database).unwrap();
        run_telemetry_migrations(&conn).unwrap();
        conn.pragma_update(None, "journal_mode", "WAL").unwrap();

        let tx = conn.unchecked_transaction().unwrap();
        for index in 0..(DELETE_BATCH_SIZE + 1) {
            tx.execute(
                "INSERT INTO telemetry_turns
                    (event_key, session_id, provider, turn_id, model, ended_at,
                     input_tokens, output_tokens, source_key, source_path)
                 VALUES (?1, 'session-a', 'codex', ?2, 'model-a',
                         '2026-01-01T00:15:00.000Z', 1, 1, 'source-a', 'log')",
                params![format!("old-event-{index}"), format!("old-turn-{index}")],
            )
            .unwrap();
        }
        tx.execute(
            "INSERT INTO telemetry_turns
                (event_key, session_id, provider, turn_id, model, ended_at,
                 input_tokens, output_tokens, source_key, source_path)
            VALUES ('new-event', 'session-a', 'codex', 'new-turn', 'model-a',
                     '2026-08-30T00:15:00.000Z', 99, 9, 'source-a', 'log')",
            [],
        )
        .unwrap();
        tx.commit().unwrap();

        let mut dirty = DirtyBuckets::new();
        mark_dirty(&mut dirty, "session-a", "2026-01-01T00:15:00.000Z");
        mark_dirty(&mut dirty, "session-a", "2026-08-30T00:15:00.000Z");
        recompute_buckets(&conn, &dirty).unwrap();

        let mut batches = 0;
        let mut interrupt_after_first_batch = || {
            batches += 1;
            if batches == 1 {
                Err(sqlite_io_error(std::io::Error::other(
                    "injected retention interruption after committed batch",
                )))
            } else {
                Ok(())
            }
        };
        let mut hook: Option<&mut dyn FnMut() -> rusqlite::Result<()>> =
            Some(&mut interrupt_after_first_batch);
        let now = DateTime::parse_from_rfc3339("2026-08-30T12:00:00.000Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(maintain_at_with_hook(&conn, 1, &first_backup, false, now, &mut hook,).is_err());
        assert!(first_backup.exists());
        assert_eq!(
            conn.query_row::<i64, _, _>(
                "SELECT count(*) FROM telemetry_turns WHERE ended_at < '2026-08-29T12:00:00.000Z'",
                [],
                |row| row.get(0),
            )
            .unwrap(),
            1
        );

        let later = DateTime::parse_from_rfc3339("2026-08-30T13:00:00.000Z")
            .unwrap()
            .with_timezone(&Utc);
        let report = maintain_at(&conn, 1, &first_backup, false, later).unwrap();
        assert_eq!(report.cutoff, "2026-08-29T12:00:00.000Z");
        assert_eq!(
            conn.query_row::<i64, _, _>(
                "SELECT count(*) FROM telemetry_turns WHERE ended_at < '2026-08-29T12:00:00.000Z'",
                [],
                |row| row.get(0),
            )
            .unwrap(),
            0
        );
        assert_eq!(
            conn.query_row::<(i64, i64), _, _>(
                "SELECT turns, input_tokens FROM telemetry_rollup_hourly
                 WHERE bucket_start = '2026-08-30T00:00:00.000Z'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap(),
            (1, 99)
        );
        assert_eq!(
            conn.query_row::<(i64, i64), _, _>(
                "SELECT turns, input_tokens FROM telemetry_rollup_hourly
                 WHERE bucket_start = '2026-01-01T00:00:00.000Z'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap(),
            (DELETE_BATCH_SIZE + 1, DELETE_BATCH_SIZE + 1)
        );
    }
}
