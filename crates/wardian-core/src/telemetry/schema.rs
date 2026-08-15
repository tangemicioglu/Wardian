//! Telemetry store schema.
//!
//! Fact tables are append-only and carry uniqueness constraints that make
//! re-ingest idempotent, because a source may legitimately be re-read after a
//! rotation, a parser version bump, or a crash between fact write and cursor
//! advance.

use rusqlite::Connection;

/// Bumping this rebuilds the telemetry tables from scratch.
///
/// That is safe in a way it would not be for most stores: every row here is
/// derived from a provider source that still exists on disk, so a rebuild costs
/// one re-ingest rather than losing data. Treating telemetry as a cache over
/// provider truth is what lets the shape change freely as sources are added.
pub const TELEMETRY_SCHEMA_VERSION: i64 = 4;

const TELEMETRY_TABLES: [&str; 6] = [
    "telemetry_rollup_hourly",
    "telemetry_limits",
    "telemetry_activity",
    "telemetry_edits",
    "telemetry_turns",
    "telemetry_sources",
];

pub fn run_telemetry_migrations(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS telemetry_meta (
            key   TEXT PRIMARY KEY,
            value INTEGER NOT NULL
        )",
        [],
    )?;

    let stored: Option<i64> = conn
        .query_row(
            "SELECT value FROM telemetry_meta WHERE key = 'schema_version'",
            [],
            |row| row.get(0),
        )
        .ok();

    if stored.is_some_and(|version| version != TELEMETRY_SCHEMA_VERSION) {
        for table in TELEMETRY_TABLES {
            conn.execute(&format!("DROP TABLE IF EXISTS {table}"), [])?;
        }
    }

    // Sources are keyed by provider, agent session, and path together.
    //
    // Path alone is wrong for any provider that keeps one database for the whole
    // machine, which opencode does: every agent's turns live in a single
    // `opencode.db`. Keyed by path, the first agent to ingest would leave its
    // high-water mark as *the* cursor, and the next agent would resume from a
    // position in someone else's history and silently skip its own.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS telemetry_sources (
            source_key          TEXT PRIMARY KEY,
            source_path         TEXT NOT NULL,
            session_id          TEXT NOT NULL,
            provider_session_id TEXT,
            provider            TEXT NOT NULL,
            source_kind         TEXT NOT NULL,
            cursor_kind         TEXT NOT NULL,
            cursor_value        INTEGER NOT NULL DEFAULT 0,
            last_size           INTEGER NOT NULL DEFAULT 0,
            last_modified       TEXT,
            last_ingested_at    TEXT,
            parser_version      INTEGER NOT NULL DEFAULT 1,
            fingerprint         TEXT,
            carry_turn_id       TEXT,
            carry_model         TEXT,
            carry_effort        TEXT,
            carry_cwd           TEXT
        )",
        [],
    )?;

    // Uniqueness rests on `event_key`, which is NOT NULL by design. The previous
    // key included the nullable `turn_id`, and SQL treats NULLs as distinct: two
    // reads of the same record would satisfy the constraint rather than collide,
    // making `INSERT OR IGNORE` insert instead of ignore.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS telemetry_turns (
            id                  INTEGER PRIMARY KEY AUTOINCREMENT,
            event_key           TEXT NOT NULL,
            session_id          TEXT NOT NULL,
            provider            TEXT NOT NULL,
            turn_id             TEXT,
            model               TEXT,
            effort              TEXT,
            started_at          TEXT,
            ended_at            TEXT NOT NULL,
            input_tokens        INTEGER,
            cached_input_tokens INTEGER,
            cache_write_tokens  INTEGER,
            output_tokens       INTEGER,
            reasoning_tokens    INTEGER,
            context_window      INTEGER,
            cost_usd            REAL,
            source_key          TEXT NOT NULL,
            source_path         TEXT NOT NULL,
            UNIQUE(source_key, event_key)
        )",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS telemetry_edits (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            event_key     TEXT NOT NULL,
            session_id    TEXT NOT NULL,
            provider      TEXT NOT NULL,
            turn_id       TEXT,
            occurred_at   TEXT NOT NULL,
            workspace     TEXT,
            path          TEXT NOT NULL,
            op            TEXT NOT NULL,
            lines_added   INTEGER,
            lines_removed INTEGER,
            source_key    TEXT NOT NULL,
            source_path   TEXT NOT NULL,
            UNIQUE(source_key, event_key)
        )",
        [],
    )?;

    // `last_event_at` is the last real event; `ended_at` is that plus the
    // singleton credit where one applies. Clustering across a delta boundary
    // resumes from the former, because resuming from the credited end would
    // measure the next gap from a moment at which nothing happened.
    // `source_key` makes intervals source-owned like every other fact. Without
    // it a re-read could not remove the intervals it was about to recompute, so
    // a parser fix or a replaced log would leave its old spans in place beside
    // the new ones and over-report active time permanently.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS telemetry_activity (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id    TEXT NOT NULL,
            provider      TEXT NOT NULL,
            started_at    TEXT NOT NULL,
            ended_at      TEXT NOT NULL,
            last_event_at TEXT NOT NULL,
            event_count   INTEGER NOT NULL,
            method        TEXT NOT NULL,
            source_key    TEXT NOT NULL DEFAULT '',
            UNIQUE(session_id, started_at)
        )",
        [],
    )?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS telemetry_limits (
            id             INTEGER PRIMARY KEY AUTOINCREMENT,
            provider       TEXT NOT NULL,
            limit_id       TEXT,
            observed_at    TEXT NOT NULL,
            used_percent   REAL,
            window_minutes INTEGER,
            resets_at      TEXT,
            plan_type      TEXT,
            UNIQUE(provider, limit_id, observed_at)
        )",
        [],
    )?;

    // Two deliberate absences.
    //
    // There is no blended `active_ms`: measured and clustered durations are
    // different kinds of quantity, and a column holding their sum would be read
    // as authoritative by every consumer downstream.
    //
    // Token columns are nullable rather than `NOT NULL DEFAULT 0`, so a
    // component no contributing turn reported stays NULL. `SUM` over all-NULL
    // input already yields NULL, so this needs no special handling — only the
    // absence of a `COALESCE` that would have manufactured a zero.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS telemetry_rollup_hourly (
            bucket_start        TEXT NOT NULL,
            session_id          TEXT NOT NULL,
            provider            TEXT NOT NULL,
            model               TEXT NOT NULL DEFAULT '',
            measured_active_ms  INTEGER NOT NULL DEFAULT 0,
            clustered_active_ms INTEGER NOT NULL DEFAULT 0,
            turns               INTEGER NOT NULL DEFAULT 0,
            input_tokens        INTEGER,
            cached_input_tokens INTEGER,
            cache_write_tokens  INTEGER,
            output_tokens       INTEGER,
            reasoning_tokens    INTEGER,
            tokens_reported     INTEGER NOT NULL DEFAULT 0,
            files_touched       INTEGER NOT NULL DEFAULT 0,
            lines_added         INTEGER NOT NULL DEFAULT 0,
            lines_removed       INTEGER NOT NULL DEFAULT 0,
            cost_usd            REAL,
            PRIMARY KEY (bucket_start, session_id, provider, model)
        )",
        [],
    )?;

    for statement in [
        "CREATE INDEX IF NOT EXISTS idx_telemetry_rollup_bucket
            ON telemetry_rollup_hourly(bucket_start)",
        "CREATE INDEX IF NOT EXISTS idx_telemetry_rollup_provider
            ON telemetry_rollup_hourly(provider, bucket_start)",
        "CREATE INDEX IF NOT EXISTS idx_telemetry_turns_session_end
            ON telemetry_turns(session_id, ended_at)",
        "CREATE INDEX IF NOT EXISTS idx_telemetry_turns_end
            ON telemetry_turns(ended_at)",
        "CREATE INDEX IF NOT EXISTS idx_telemetry_edits_session_time
            ON telemetry_edits(session_id, occurred_at)",
        "CREATE INDEX IF NOT EXISTS idx_telemetry_edits_time
            ON telemetry_edits(occurred_at)",
        "CREATE INDEX IF NOT EXISTS idx_telemetry_activity_session
            ON telemetry_activity(session_id, started_at)",
        "CREATE INDEX IF NOT EXISTS idx_telemetry_activity_start
            ON telemetry_activity(started_at)",
        "CREATE INDEX IF NOT EXISTS idx_telemetry_limits_provider
            ON telemetry_limits(provider, observed_at)",
    ] {
        conn.execute(statement, [])?;
    }

    conn.execute(
        "INSERT INTO telemetry_meta (key, value) VALUES ('schema_version', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [TELEMETRY_SCHEMA_VERSION],
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table_names(conn: &Connection) -> Vec<String> {
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name LIKE 'telemetry%' ORDER BY name")
            .unwrap();
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        rows
    }

    #[test]
    fn migrations_create_every_table() {
        let conn = Connection::open_in_memory().unwrap();
        run_telemetry_migrations(&conn).unwrap();
        assert_eq!(
            table_names(&conn),
            vec![
                "telemetry_activity",
                "telemetry_edits",
                "telemetry_limits",
                "telemetry_meta",
                "telemetry_rollup_hourly",
                "telemetry_sources",
                "telemetry_turns",
            ]
        );
    }

    #[test]
    fn migrations_are_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        run_telemetry_migrations(&conn).unwrap();
        run_telemetry_migrations(&conn).unwrap();
        assert_eq!(table_names(&conn).len(), 7);
    }

    #[test]
    fn an_older_schema_version_is_rebuilt_rather_than_read() {
        let conn = Connection::open_in_memory().unwrap();
        run_telemetry_migrations(&conn).unwrap();
        conn.execute(
            "INSERT INTO telemetry_turns (event_key, session_id, provider, ended_at, source_key, source_path)
             VALUES ('k', 's', 'codex', '2026-08-13T00:00:00Z', 'sk', 'log.jsonl')",
            [],
        )
        .unwrap();
        conn.execute(
            "UPDATE telemetry_meta SET value = 1 WHERE key = 'schema_version'",
            [],
        )
        .unwrap();

        run_telemetry_migrations(&conn).unwrap();

        // Rebuilt, not carried across: rows from an older shape would otherwise
        // be read through the new one's assumptions.
        let turns: i64 = conn
            .query_row("SELECT count(*) FROM telemetry_turns", [], |row| row.get(0))
            .unwrap();
        assert_eq!(turns, 0);
        let version: i64 = conn
            .query_row(
                "SELECT value FROM telemetry_meta WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, TELEMETRY_SCHEMA_VERSION);
    }

    #[test]
    fn duplicate_turn_is_rejected() {
        let conn = Connection::open_in_memory().unwrap();
        run_telemetry_migrations(&conn).unwrap();
        let insert = "INSERT INTO telemetry_turns (event_key, session_id, provider, turn_id, ended_at, source_key, source_path)
                      VALUES ('e1', 's', 'codex', 't1', '2026-08-13T00:00:00Z', 'sk', 'log.jsonl')";
        conn.execute(insert, []).unwrap();
        // Re-ingest of the same delta must not double count.
        assert!(conn.execute(insert, []).is_err());
    }

    #[test]
    fn a_turn_without_a_turn_id_still_deduplicates() {
        // The regression this guards: with the old key, a NULL `turn_id` made
        // every re-read of the same record insert a fresh row, because SQL
        // compares NULLs as distinct.
        let conn = Connection::open_in_memory().unwrap();
        run_telemetry_migrations(&conn).unwrap();
        let insert = "INSERT INTO telemetry_turns (event_key, session_id, provider, turn_id, ended_at, source_key, source_path)
                      VALUES ('e1', 's', 'codex', NULL, '2026-08-13T00:00:00Z', 'sk', 'log.jsonl')";
        conn.execute(insert, []).unwrap();
        assert!(conn.execute(insert, []).is_err());
    }

    #[test]
    fn the_same_record_read_with_and_without_turn_context_collides() {
        // An incremental read can miss the `turn_context` a full re-read sees.
        // Both must resolve to one row, since they describe one event.
        let conn = Connection::open_in_memory().unwrap();
        run_telemetry_migrations(&conn).unwrap();
        conn.execute(
            "INSERT INTO telemetry_turns (event_key, session_id, provider, turn_id, ended_at, source_key, source_path)
             VALUES ('e1', 's', 'codex', NULL, '2026-08-13T00:00:00Z', 'sk', 'log.jsonl')",
            [],
        )
        .unwrap();
        assert!(conn
            .execute(
                "INSERT INTO telemetry_turns (event_key, session_id, provider, turn_id, ended_at, source_key, source_path)
                 VALUES ('e1', 's', 'codex', 't1', '2026-08-13T00:00:00Z', 'sk', 'log.jsonl')",
                [],
            )
            .is_err());
    }

    #[test]
    fn two_sessions_sharing_one_database_keep_separate_cursors() {
        // opencode keeps one database for the whole machine, so a path-keyed
        // cursor would let one agent's high-water mark skip another's history.
        let conn = Connection::open_in_memory().unwrap();
        run_telemetry_migrations(&conn).unwrap();
        for (key, session, cursor) in [
            ("codex|agent-a|db", "agent-a", 5_000),
            ("codex|agent-b|db", "agent-b", 1_000),
        ] {
            conn.execute(
                "INSERT INTO telemetry_sources
                     (source_key, source_path, session_id, provider, source_kind, cursor_kind, cursor_value)
                 VALUES (?1, 'opencode.db', ?2, 'opencode', 'sqlite', 'epoch_ms', ?3)",
                rusqlite::params![key, session, cursor],
            )
            .unwrap();
        }
        let cursors: i64 = conn
            .query_row(
                "SELECT count(DISTINCT cursor_value) FROM telemetry_sources WHERE source_path = 'opencode.db'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(cursors, 2);
    }

    #[test]
    fn duplicate_activity_interval_is_rejected() {
        let conn = Connection::open_in_memory().unwrap();
        run_telemetry_migrations(&conn).unwrap();
        let insert = "INSERT INTO telemetry_activity
                          (session_id, provider, started_at, ended_at, last_event_at, event_count, method)
                      VALUES ('s', 'codex', '2026-08-13T00:00:00Z', '2026-08-13T00:10:00Z',
                              '2026-08-13T00:10:00Z', 5, 'clustered')";
        conn.execute(insert, []).unwrap();
        assert!(conn.execute(insert, []).is_err());
    }
}
