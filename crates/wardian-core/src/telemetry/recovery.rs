//! Recovery for telemetry facts written under the wrong agent.
//!
//! A file-backed Codex or Claude source has one physical identity, while its
//! stored owner is the Wardian agent whose recorded provider session owns that
//! file. Older discovery code could visit one physical file through multiple
//! agent projections and rewrite the source owner between passes. This module
//! repairs the resulting fact rows from that canonical source ownership rather
//! than trying to correct a dashboard query.

use crate::telemetry::identity::canonical_path;
use crate::telemetry::maintenance::create_verified_backup;
use crate::telemetry::rollup::rebuild_all_rollups;
use crate::telemetry::schema::{
    acquire_telemetry_lock, ensure_string_id, run_telemetry_migrations, sqlite_io_error,
    STRING_SESSION,
};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::Serialize;
use std::io;
use std::path::Path;

/// Current source-attribution health, suitable for a dry run or post-repair
/// verification.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct AttributionStatus {
    pub sources_examined: i64,
    pub foreign_turns: i64,
    pub foreign_edits: i64,
    pub foreign_activity: i64,
    pub unowned_turns: i64,
    pub unowned_edits: i64,
    pub unowned_activity: i64,
}

/// Effects and before/after evidence from one attribution repair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AttributionRepairReport {
    pub before: AttributionStatus,
    pub after: AttributionStatus,
    pub turns_reattributed: i64,
    pub edits_reattributed: i64,
    pub activity_reattributed: i64,
    pub activity_duplicates_removed: i64,
    pub buckets_recomputed: usize,
}

/// Inspect attribution without migrating or writing the database.
pub fn inspect_attribution(conn: &Connection) -> rusqlite::Result<AttributionStatus> {
    Ok(AttributionStatus {
        sources_examined: conn.query_row("SELECT COUNT(*) FROM telemetry_sources", [], |row| {
            row.get(0)
        })?,
        foreign_turns: count(
            conn,
            "
            SELECT COUNT(*)
            FROM telemetry_turns facts
            JOIN telemetry_sources sources ON sources.source_key = facts.source_key
            WHERE facts.session_id <> sources.session_id
        ",
        )?,
        foreign_edits: count(
            conn,
            "
            SELECT COUNT(*)
            FROM telemetry_edits facts
            JOIN telemetry_sources sources ON sources.source_key = facts.source_key
            WHERE facts.session_id <> sources.session_id
        ",
        )?,
        foreign_activity: count(
            conn,
            "
            SELECT COUNT(*)
            FROM telemetry_activity facts
            JOIN telemetry_sources sources ON sources.source_key = facts.source_key
            WHERE facts.session_id <> sources.session_id
        ",
        )?,
        unowned_turns: count(
            conn,
            "
            SELECT COUNT(*)
            FROM telemetry_turns facts
            LEFT JOIN telemetry_sources sources ON sources.source_key = facts.source_key
            WHERE sources.source_key IS NULL
        ",
        )?,
        unowned_edits: count(
            conn,
            "
            SELECT COUNT(*)
            FROM telemetry_edits facts
            LEFT JOIN telemetry_sources sources ON sources.source_key = facts.source_key
            WHERE sources.source_key IS NULL
        ",
        )?,
        unowned_activity: count(
            conn,
            "
            SELECT COUNT(*)
            FROM telemetry_activity facts
            LEFT JOIN telemetry_sources sources ON sources.source_key = facts.source_key
            WHERE sources.source_key IS NULL
        ",
        )?,
    })
}

/// Back up, repair, and verify telemetry attribution.
///
/// `backup_path` is mandatory because the operation changes persisted facts.
/// A missing backup is created with `VACUUM INTO`, integrity-checked, and
/// atomically renamed into place. An existing backup is integrity-checked and
/// reused rather than overwritten, so retries preserve the original recovery
/// point. The repair itself is one SQLite transaction and is safe to rerun.
pub fn repair_attribution(
    conn: &Connection,
    backup_path: &Path,
) -> rusqlite::Result<AttributionRepairReport> {
    validate_backup_target(conn, backup_path)?;
    if let Some(parent) = backup_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent).map_err(sqlite_io_error)?;
    }

    // The backup must precede telemetry migrations: an old installed database
    // may still be changed while being upgraded to the normalized schema.
    {
        let _telemetry_lock = acquire_telemetry_lock(conn)?;
        create_verified_backup(conn, backup_path)?;
    }
    run_telemetry_migrations(conn)?;
    let _telemetry_lock = acquire_telemetry_lock(conn)?;

    let before = inspect_attribution(conn)?;
    reject_unowned(&before)?;
    if before.foreign_turns == 0 && before.foreign_edits == 0 && before.foreign_activity == 0 {
        return Ok(AttributionRepairReport {
            after: before.clone(),
            before,
            turns_reattributed: 0,
            edits_reattributed: 0,
            activity_reattributed: 0,
            activity_duplicates_removed: 0,
            buckets_recomputed: 0,
        });
    }

    let tx = rusqlite::Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    let sources = source_owners(&tx)?;
    let mut turns_reattributed = 0;
    let mut edits_reattributed = 0;
    let mut activity_reattributed = 0;
    let mut activity_duplicates_removed = 0;

    for (source_key, owner) in sources {
        let owner_ref = ensure_string_id(&tx, STRING_SESSION, &owner)?;
        let source_ref = tx
            .query_row(
                "SELECT string_id FROM telemetry_strings
                 WHERE kind = 'source_key' AND value = ?1",
                params![source_key],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .ok_or_else(|| invalid_request("telemetry source has no normalized source key"))?;

        turns_reattributed += tx.execute(
            "UPDATE telemetry_turn_facts
             SET session_ref = ?1
             WHERE source_ref = ?2 AND session_ref <> ?1",
            params![owner_ref, source_ref],
        )? as i64;
        edits_reattributed += tx.execute(
            "UPDATE telemetry_edit_facts
             SET session_ref = ?1
             WHERE source_ref = ?2 AND session_ref <> ?1",
            params![owner_ref, source_ref],
        )? as i64;

        activity_duplicates_removed += tx.execute(
            "DELETE FROM telemetry_activity AS foreign_fact
             WHERE foreign_fact.source_key = ?1
               AND foreign_fact.session_id <> ?2
               AND EXISTS (
                   SELECT 1 FROM telemetry_activity valid_fact
                   WHERE valid_fact.session_id = ?2
                     AND valid_fact.started_at = foreign_fact.started_at
               )",
            params![source_key, owner],
        )? as i64;
        activity_reattributed += tx.execute(
            "UPDATE telemetry_activity
             SET session_id = ?1
             WHERE source_key = ?2 AND session_id <> ?1",
            params![owner, source_key],
        )? as i64;
    }

    let buckets_recomputed = rebuild_all_rollups(&tx)?;
    let in_transaction = inspect_attribution(&tx)?;
    reject_unowned(&in_transaction)?;
    if in_transaction.foreign_turns != 0
        || in_transaction.foreign_edits != 0
        || in_transaction.foreign_activity != 0
    {
        return Err(invalid_request(
            "telemetry attribution repair left foreign facts behind",
        ));
    }
    tx.commit()?;

    let after = inspect_attribution(conn)?;
    reject_unowned(&after)?;
    if after.foreign_turns != 0 || after.foreign_edits != 0 || after.foreign_activity != 0 {
        return Err(invalid_request(
            "telemetry attribution repair left foreign facts behind",
        ));
    }

    Ok(AttributionRepairReport {
        before,
        after,
        turns_reattributed,
        edits_reattributed,
        activity_reattributed,
        activity_duplicates_removed,
        buckets_recomputed,
    })
}

fn count(conn: &Connection, sql: &str) -> rusqlite::Result<i64> {
    conn.query_row(sql, [], |row| row.get(0))
}

fn source_owners(conn: &Connection) -> rusqlite::Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT source_key, session_id
         FROM telemetry_sources
         ORDER BY source_key",
    )?;
    let owners = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect();
    owners
}

fn reject_unowned(status: &AttributionStatus) -> rusqlite::Result<()> {
    let unowned = status.unowned_turns + status.unowned_edits + status.unowned_activity;
    if unowned == 0 {
        return Ok(());
    }
    Err(invalid_request(format!(
        "telemetry repair found {unowned} facts without a canonical source owner; no facts were changed"
    )))
}

fn validate_backup_target(conn: &Connection, backup_path: &Path) -> rusqlite::Result<()> {
    if backup_path.as_os_str().is_empty() {
        return Err(invalid_request("telemetry backup path must not be empty"));
    }
    let database_path: String = conn.query_row("PRAGMA database_list", [], |row| row.get(2))?;
    if database_path != ":memory:" && !database_path.is_empty() {
        let database_path = canonical_path(Path::new(&database_path));
        let backup_path = canonical_path(backup_path);
        if database_path == backup_path {
            return Err(invalid_request(
                "telemetry backup path must differ from the database path",
            ));
        }
    }
    Ok(())
}

fn invalid_request(message: impl Into<String>) -> rusqlite::Error {
    sqlite_io_error(io::Error::new(io::ErrorKind::InvalidInput, message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::identity::source_key as make_source_key;
    use crate::telemetry::query::{breakdown, Dimension};
    use crate::telemetry::schema::run_telemetry_migrations;
    use crate::telemetry::verify_backup;
    use rusqlite::params;
    use tempfile::tempdir;

    fn insert_source(conn: &Connection, key: &str, path: &str, owner: &str) {
        conn.execute(
            "INSERT INTO telemetry_sources(
                 source_key, source_path, session_id, provider, source_kind,
                 cursor_kind, cursor_value, parser_version
             ) VALUES (?1,?2,?3,'codex','jsonl','byte_offset',0,2)",
            params![key, path, owner],
        )
        .unwrap();
    }

    fn insert_turn(conn: &Connection, key: &str, session: &str, event: &str, input: i64) {
        conn.execute(
            "INSERT INTO telemetry_turns(
                 event_key, session_id, provider, turn_id, ended_at,
                 input_tokens, output_tokens, source_key, source_path
             ) VALUES (?1,?2,'codex',?1,'2026-09-01T10:15:00Z',?3,1,?4,'rollout.jsonl')",
            params![event, session, input, key],
        )
        .unwrap();
    }

    fn insert_edit(conn: &Connection, key: &str, session: &str, event: &str, added: i64) {
        conn.execute(
            "INSERT INTO telemetry_edits(
                 event_key, session_id, provider, occurred_at, path, op,
                 lines_added, source_key, source_path
             ) VALUES (?1,?2,'codex','2026-09-01T10:15:00Z','src/lib.rs','modify',?3,?4,'rollout.jsonl')",
            params![event, session, added, key],
        )
        .unwrap();
    }

    fn insert_activity(conn: &Connection, key: &str, session: &str, started_at: &str) {
        conn.execute(
            "INSERT INTO telemetry_activity(
                 session_id, provider, started_at, ended_at, last_event_at,
                 event_count, method, source_key
             ) VALUES (?1,'codex',?2,'2026-09-01T10:20:00Z',?2,2,'measured',?3)",
            params![session, started_at, key],
        )
        .unwrap();
    }

    #[test]
    fn repair_preserves_valid_facts_rebuilds_differences_and_is_idempotent() {
        let temp = tempdir().unwrap();
        let db_path = temp.path().join("state.db");
        let backup_path = temp.path().join("state.db.before-attribution-repair");
        let conn = Connection::open(&db_path).unwrap();
        run_telemetry_migrations(&conn).unwrap();

        let source_a = make_source_key("codex", "agent-a", "rollout-a.jsonl");
        let source_b = make_source_key("codex", "agent-b", "rollout-b.jsonl");
        insert_source(&conn, &source_a, "rollout-a.jsonl", "agent-a");
        insert_source(&conn, &source_b, "rollout-b.jsonl", "agent-b");

        insert_turn(&conn, &source_a, "agent-a", "valid-turn", 10);
        insert_turn(&conn, &source_a, "foreign-turn", "foreign-turn", 100);
        insert_turn(&conn, &source_b, "agent-b", "other-turn", 20);
        insert_edit(&conn, &source_a, "agent-a", "valid-edit", 3);
        insert_edit(&conn, &source_a, "foreign-edit", "foreign-edit", 30);
        insert_edit(&conn, &source_b, "agent-b", "other-edit", 4);
        insert_activity(&conn, &source_a, "agent-a", "2026-09-01T10:00:00Z");
        insert_activity(&conn, &source_a, "agent-foreign", "2026-09-01T10:00:00Z");
        insert_activity(&conn, &source_a, "agent-foreign", "2026-09-01T10:10:00Z");
        insert_activity(&conn, &source_b, "agent-b", "2026-09-01T10:15:00Z");

        let before = inspect_attribution(&conn).unwrap();
        assert_eq!(before.foreign_turns, 1);
        assert_eq!(before.foreign_edits, 1);
        assert_eq!(before.foreign_activity, 2);

        let report = repair_attribution(&conn, &backup_path).unwrap();
        assert_eq!(report.before, before);
        assert_eq!(report.turns_reattributed, 1);
        assert_eq!(report.edits_reattributed, 1);
        assert_eq!(report.activity_duplicates_removed, 1);
        assert_eq!(report.activity_reattributed, 1);
        assert_eq!(report.after.foreign_turns, 0);
        assert_eq!(report.after.foreign_edits, 0);
        assert_eq!(report.after.foreign_activity, 0);
        assert!(backup_path.exists());
        verify_backup(&backup_path).unwrap();
        let backup = Connection::open(&backup_path).unwrap();
        let backup_status = inspect_attribution(&backup).unwrap();
        assert_eq!(backup_status, before);

        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM telemetry_turns WHERE session_id = 'agent-a'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            2
        );
        assert_eq!(
            conn.query_row(
                "SELECT COUNT(*) FROM telemetry_activity WHERE session_id = 'agent-b'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap(),
            1
        );

        let rows = breakdown(
            &conn,
            Dimension::Agent,
            "2026-09-01T00:00:00Z",
            "2026-09-02T00:00:00Z",
            24,
        )
        .unwrap();
        let agent_a = rows.iter().find(|row| row.key == "agent-a").unwrap();
        let agent_b = rows.iter().find(|row| row.key == "agent-b").unwrap();
        assert_ne!(agent_a.turns, agent_b.turns);
        assert_ne!(agent_a.lines_added, agent_b.lines_added);

        let second = repair_attribution(&conn, &backup_path).unwrap();
        assert_eq!(second.before.foreign_turns, 0);
        assert_eq!(second.before.foreign_edits, 0);
        assert_eq!(second.before.foreign_activity, 0);
        assert_eq!(second.turns_reattributed, 0);
        assert_eq!(second.edits_reattributed, 0);
        assert_eq!(second.activity_reattributed, 0);
        assert_eq!(second.activity_duplicates_removed, 0);
        assert_eq!(second.buckets_recomputed, 0);
    }
}
