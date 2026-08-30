//! Telemetry store schema.
//!
//! Fact tables are append-only and carry uniqueness constraints that make
//! re-ingest idempotent, because a source may legitimately be re-read after a
//! rotation, a parser version bump, or a crash between fact write and cursor
//! advance.

use fs2::FileExt;
use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use std::fs::{File, OpenOptions};
use std::io;
use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

/// The normalized telemetry schema version.
///
/// Version 4 is migrated in resumable batches. New fact rows store integer
/// references to repeated strings; compatibility views keep the read-facing
/// column names stable.
pub const TELEMETRY_SCHEMA_VERSION: i64 = 5;

const LEGACY_TELEMETRY_SCHEMA_VERSION: i64 = 4;
const COPY_BATCH_SIZE: i64 = 2_000;
const TURN_PROGRESS_KEY: &str = "normalization_turn_last_id";
const EDIT_PROGRESS_KEY: &str = "normalization_edit_last_id";
const TELEMETRY_LOCK_TIMEOUT: Duration = Duration::from_secs(30);
const TELEMETRY_LOCK_POLL: Duration = Duration::from_millis(50);

const NORMALIZED_TABLES: [&str; 4] = [
    "telemetry_source_paths",
    "telemetry_turn_facts",
    "telemetry_edit_facts",
    "telemetry_strings",
];

pub(crate) const STRING_SESSION: &str = "session";
pub(crate) const STRING_PROVIDER: &str = "provider";
pub(crate) const STRING_MODEL: &str = "model";
pub(crate) const STRING_EFFORT: &str = "effort";
const STRING_SOURCE_KEY: &str = "source_key";

const TELEMETRY_TABLES: [&str; 6] = [
    "telemetry_rollup_hourly",
    "telemetry_limits",
    "telemetry_activity",
    "telemetry_edits",
    "telemetry_turns",
    "telemetry_sources",
];

pub fn run_telemetry_migrations(conn: &Connection) -> rusqlite::Result<()> {
    conn.busy_timeout(TELEMETRY_LOCK_TIMEOUT)?;
    // SQLite releases a transaction lock after every copy batch so progress is
    // durable and restartable. A transaction alone therefore cannot serialize
    // two processes: both could observe v4 and copy the same batch. This OS
    // lock is the short-lived migration lease for current app/CLI processes;
    // the legacy-writer fence is installed only once a v4 schema is observed.
    let _telemetry_lock = acquire_telemetry_lock(conn)?;
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

    if stored == Some(TELEMETRY_SCHEMA_VERSION) {
        create_normalized_schema(conn)?;
        ensure_normalized_indexes(conn)?;
        ensure_compatibility_views(conn)?;
        return Ok(());
    }

    if stored == Some(LEGACY_TELEMETRY_SCHEMA_VERSION) {
        migrate_legacy_schema(conn)?;
        return Ok(());
    }

    if stored.is_some() {
        reset_telemetry_schema(conn)?;
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

    // The bootstrap path creates empty legacy-shaped tables in the same
    // connection, so there is no older writer to fence here. Reserve the
    // exclusive locking-mode fence for the actual v4-to-v5 upgrade above.
    let mut no_hook: Option<&mut dyn FnMut() -> rusqlite::Result<()>> = None;
    migrate_legacy_schema_unlocked(conn, &mut no_hook)
}

fn create_normalized_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS telemetry_strings (
            string_id INTEGER PRIMARY KEY AUTOINCREMENT,
            kind      TEXT NOT NULL,
            value     TEXT NOT NULL,
            UNIQUE(kind, value)
        )",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS telemetry_source_paths (
            source_ref  INTEGER PRIMARY KEY REFERENCES telemetry_strings(string_id),
            source_path TEXT NOT NULL
        )",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS telemetry_turn_facts (
            id                  INTEGER PRIMARY KEY AUTOINCREMENT,
            event_key           TEXT NOT NULL,
            session_ref         INTEGER NOT NULL REFERENCES telemetry_strings(string_id),
            provider_ref        INTEGER NOT NULL REFERENCES telemetry_strings(string_id),
            turn_id             TEXT,
            model_ref           INTEGER REFERENCES telemetry_strings(string_id),
            effort_ref          INTEGER REFERENCES telemetry_strings(string_id),
            started_at          TEXT,
            ended_at            TEXT NOT NULL,
            input_tokens        INTEGER,
            cached_input_tokens INTEGER,
            cache_write_tokens  INTEGER,
            output_tokens       INTEGER,
            reasoning_tokens    INTEGER,
            context_window      INTEGER,
            cost_usd            REAL,
            source_ref          INTEGER NOT NULL REFERENCES telemetry_strings(string_id)
        )",
        [],
    )?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS telemetry_edit_facts (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            event_key     TEXT NOT NULL,
            session_ref   INTEGER NOT NULL REFERENCES telemetry_strings(string_id),
            provider_ref  INTEGER NOT NULL REFERENCES telemetry_strings(string_id),
            turn_id       TEXT,
            occurred_at   TEXT NOT NULL,
            workspace     TEXT,
            path          TEXT NOT NULL,
            op            TEXT NOT NULL,
            lines_added   INTEGER,
            lines_removed INTEGER,
            source_ref    INTEGER NOT NULL REFERENCES telemetry_strings(string_id)
        )",
        [],
    )?;
    Ok(())
}

fn ensure_normalized_indexes(conn: &Connection) -> rusqlite::Result<()> {
    for statement in [
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_telemetry_turn_facts_source_event
            ON telemetry_turn_facts(source_ref, event_key)",
        "CREATE INDEX IF NOT EXISTS idx_telemetry_turn_facts_session_end
            ON telemetry_turn_facts(session_ref, ended_at)",
        "CREATE INDEX IF NOT EXISTS idx_telemetry_turn_facts_end
            ON telemetry_turn_facts(ended_at)",
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_telemetry_edit_facts_source_event
            ON telemetry_edit_facts(source_ref, event_key)",
        "CREATE INDEX IF NOT EXISTS idx_telemetry_edit_facts_session_time
            ON telemetry_edit_facts(session_ref, occurred_at)",
        "CREATE INDEX IF NOT EXISTS idx_telemetry_edit_facts_time
            ON telemetry_edit_facts(occurred_at)",
    ] {
        conn.execute(statement, [])?;
    }
    Ok(())
}

fn ensure_compatibility_views(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute(
        "CREATE VIEW IF NOT EXISTS telemetry_turns AS
         SELECT t.id, t.event_key, session.value AS session_id,
                provider.value AS provider, t.turn_id, model.value AS model,
                effort.value AS effort, t.started_at, t.ended_at,
                t.input_tokens, t.cached_input_tokens, t.cache_write_tokens,
                t.output_tokens, t.reasoning_tokens, t.context_window,
                t.cost_usd, source.value AS source_key,
                COALESCE(paths.source_path, '') AS source_path
         FROM telemetry_turn_facts t
         JOIN telemetry_strings session
           ON session.string_id = t.session_ref AND session.kind = 'session'
         JOIN telemetry_strings provider
           ON provider.string_id = t.provider_ref AND provider.kind = 'provider'
         LEFT JOIN telemetry_strings model
           ON model.string_id = t.model_ref AND model.kind = 'model'
         LEFT JOIN telemetry_strings effort
           ON effort.string_id = t.effort_ref AND effort.kind = 'effort'
         JOIN telemetry_strings source
           ON source.string_id = t.source_ref AND source.kind = 'source_key'
         LEFT JOIN telemetry_source_paths paths
           ON paths.source_ref = t.source_ref",
        [],
    )?;
    conn.execute(
        "CREATE VIEW IF NOT EXISTS telemetry_edits AS
         SELECT e.id, e.event_key, session.value AS session_id,
                provider.value AS provider, e.turn_id, e.occurred_at,
                e.workspace, e.path, e.op, e.lines_added, e.lines_removed,
                source.value AS source_key,
                COALESCE(paths.source_path, '') AS source_path
         FROM telemetry_edit_facts e
         JOIN telemetry_strings session
           ON session.string_id = e.session_ref AND session.kind = 'session'
         JOIN telemetry_strings provider
           ON provider.string_id = e.provider_ref AND provider.kind = 'provider'
         JOIN telemetry_strings source
           ON source.string_id = e.source_ref AND source.kind = 'source_key'
         LEFT JOIN telemetry_source_paths paths
           ON paths.source_ref = e.source_ref",
        [],
    )?;
    conn.execute(
        "CREATE TRIGGER IF NOT EXISTS telemetry_turns_insert
         INSTEAD OF INSERT ON telemetry_turns
         BEGIN
           INSERT OR IGNORE INTO telemetry_strings(kind, value)
             SELECT 'session', NEW.session_id WHERE NEW.session_id IS NOT NULL;
           INSERT OR IGNORE INTO telemetry_strings(kind, value)
             SELECT 'provider', NEW.provider WHERE NEW.provider IS NOT NULL;
           INSERT OR IGNORE INTO telemetry_strings(kind, value)
             SELECT 'model', NEW.model WHERE NEW.model IS NOT NULL;
           INSERT OR IGNORE INTO telemetry_strings(kind, value)
             SELECT 'effort', NEW.effort WHERE NEW.effort IS NOT NULL;
           INSERT OR IGNORE INTO telemetry_strings(kind, value)
             SELECT 'source_key', NEW.source_key WHERE NEW.source_key IS NOT NULL;
           INSERT OR IGNORE INTO telemetry_source_paths(source_ref, source_path)
             SELECT string_id, COALESCE(NEW.source_path, '')
             FROM telemetry_strings
             WHERE kind = 'source_key' AND value = NEW.source_key;
           INSERT OR IGNORE INTO telemetry_turn_facts(
             event_key, session_ref, provider_ref, turn_id, model_ref, effort_ref,
             started_at, ended_at, input_tokens, cached_input_tokens,
             cache_write_tokens, output_tokens, reasoning_tokens, context_window,
             cost_usd, source_ref
           ) SELECT NEW.event_key,
                    (SELECT string_id FROM telemetry_strings WHERE kind = 'session' AND value = NEW.session_id),
                    (SELECT string_id FROM telemetry_strings WHERE kind = 'provider' AND value = NEW.provider),
                    NEW.turn_id,
                    (SELECT string_id FROM telemetry_strings WHERE kind = 'model' AND value = NEW.model),
                    (SELECT string_id FROM telemetry_strings WHERE kind = 'effort' AND value = NEW.effort),
                    NEW.started_at, NEW.ended_at, NEW.input_tokens,
                    NEW.cached_input_tokens, NEW.cache_write_tokens, NEW.output_tokens,
                    NEW.reasoning_tokens, NEW.context_window, NEW.cost_usd,
                    (SELECT string_id FROM telemetry_strings WHERE kind = 'source_key' AND value = NEW.source_key);
         END",
        [],
    )?;
    conn.execute(
        "CREATE TRIGGER IF NOT EXISTS telemetry_edits_insert
         INSTEAD OF INSERT ON telemetry_edits
         BEGIN
           INSERT OR IGNORE INTO telemetry_strings(kind, value)
             SELECT 'session', NEW.session_id WHERE NEW.session_id IS NOT NULL;
           INSERT OR IGNORE INTO telemetry_strings(kind, value)
             SELECT 'provider', NEW.provider WHERE NEW.provider IS NOT NULL;
           INSERT OR IGNORE INTO telemetry_strings(kind, value)
             SELECT 'source_key', NEW.source_key WHERE NEW.source_key IS NOT NULL;
           INSERT OR IGNORE INTO telemetry_source_paths(source_ref, source_path)
             SELECT string_id, COALESCE(NEW.source_path, '')
             FROM telemetry_strings
             WHERE kind = 'source_key' AND value = NEW.source_key;
           INSERT OR IGNORE INTO telemetry_edit_facts(
             event_key, session_ref, provider_ref, turn_id, occurred_at, workspace,
             path, op, lines_added, lines_removed, source_ref
           ) SELECT NEW.event_key,
                    (SELECT string_id FROM telemetry_strings WHERE kind = 'session' AND value = NEW.session_id),
                    (SELECT string_id FROM telemetry_strings WHERE kind = 'provider' AND value = NEW.provider),
                    NEW.turn_id, NEW.occurred_at, NEW.workspace, NEW.path, NEW.op,
                    NEW.lines_added, NEW.lines_removed,
                    (SELECT string_id FROM telemetry_strings WHERE kind = 'source_key' AND value = NEW.source_key);
         END",
        [],
    )?;
    Ok(())
}

pub(crate) fn ensure_string_id(
    conn: &Connection,
    kind: &str,
    value: &str,
) -> rusqlite::Result<i64> {
    conn.execute(
        "INSERT OR IGNORE INTO telemetry_strings(kind, value) VALUES (?1, ?2)",
        params![kind, value],
    )?;
    conn.query_row(
        "SELECT string_id FROM telemetry_strings WHERE kind = ?1 AND value = ?2",
        params![kind, value],
        |row| row.get(0),
    )
}

pub(crate) fn ensure_source_ref(
    conn: &Connection,
    source_key: &str,
    source_path: &str,
) -> rusqlite::Result<i64> {
    let source_ref = ensure_string_id(conn, STRING_SOURCE_KEY, source_key)?;
    conn.execute(
        "INSERT OR IGNORE INTO telemetry_source_paths(source_ref, source_path)
         VALUES (?1, ?2)",
        params![source_ref, source_path],
    )?;
    Ok(source_ref)
}

/// Acquire the inter-process lease shared by schema migration and maintenance.
///
/// SQLite has no advisory lock that survives the short transactions used by
/// the resumable copy. The adjacent lock file supplies that missing lease and
/// is safe across processes on the supported filesystems. A bounded wait keeps
/// a stale or inaccessible database from hanging startup forever.
pub(crate) fn acquire_telemetry_lock(conn: &Connection) -> rusqlite::Result<Option<File>> {
    let database_path =
        conn.query_row("PRAGMA database_list", [], |row| row.get::<_, String>(2))?;
    if database_path.is_empty() || database_path == ":memory:" {
        return Ok(None);
    }

    let lock_path = PathBuf::from(format!("{database_path}.telemetry-maintenance.lock"));
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(sqlite_io_error)?;
    let deadline = Instant::now() + TELEMETRY_LOCK_TIMEOUT;
    loop {
        match file.try_lock_exclusive() {
            Ok(()) => return Ok(Some(file)),
            Err(error) if lock_is_contended(&error) && Instant::now() < deadline => {
                thread::sleep(TELEMETRY_LOCK_POLL);
            }
            Err(_error) if lock_is_contended(&_error) => {
                return Err(sqlite_io_error(io::Error::new(
                    io::ErrorKind::TimedOut,
                    format!("timed out acquiring telemetry maintenance lock {lock_path:?}"),
                )));
            }
            Err(error) => return Err(sqlite_io_error(error)),
        }
    }
}

fn lock_is_contended(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::WouldBlock
        // Windows reports these sharing/lock violations as generic I/O errors.
        || matches!(error.raw_os_error(), Some(11 | 32 | 33))
}

pub(crate) fn sqlite_io_error(error: io::Error) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(error))
}

fn migrate_legacy_schema(conn: &Connection) -> rusqlite::Result<()> {
    let mut no_hook: Option<&mut dyn FnMut() -> rusqlite::Result<()>> = None;
    migrate_legacy_schema_with_hook(conn, &mut no_hook)
}

fn migrate_legacy_schema_with_hook(
    conn: &Connection,
    batch_hook: &mut Option<&mut dyn FnMut() -> rusqlite::Result<()>>,
) -> rusqlite::Result<()> {
    // A pre-fix v4 process does not know about the adjacent lease. Keep an
    // SQLite exclusive locking-mode fence across the committed copy batches
    // so that such a writer cannot mutate the legacy tables between batches.
    // The batch transactions still commit their progress, so an interruption
    // releases the fence and the next process resumes from the last marker.
    let quiescence = ExclusiveTelemetryLock::acquire(conn)?;
    let result = migrate_legacy_schema_unlocked(conn, batch_hook);
    let release_result = quiescence.release();
    result.and(release_result)
}

fn migrate_legacy_schema_unlocked(
    conn: &Connection,
    batch_hook: &mut Option<&mut dyn FnMut() -> rusqlite::Result<()>>,
) -> rusqlite::Result<()> {
    create_normalized_schema(conn)?;
    // Install the dedupe indexes before the first resumable batch. This makes
    // recovery safe even if an older process ignored the OS lease, and keeps a
    // repeated batch from ever creating duplicate facts.
    ensure_normalized_indexes(conn)?;
    seed_source_lookups(conn)?;
    copy_fact_table(
        conn,
        "telemetry_turns",
        "telemetry_turn_facts",
        TURN_PROGRESS_KEY,
        true,
        batch_hook,
    )?;
    copy_fact_table(
        conn,
        "telemetry_edits",
        "telemetry_edit_facts",
        EDIT_PROGRESS_KEY,
        false,
        batch_hook,
    )?;
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    // Recheck while holding the final write lock. A legacy writer that raced
    // the migration can only be admitted before this point; a mismatch keeps
    // the old tables intact for a later retry.
    verify_migrated_counts(&tx)?;
    tx.execute("DROP TABLE telemetry_turns", [])?;
    tx.execute("DROP TABLE telemetry_edits", [])?;
    ensure_compatibility_views(&tx)?;
    tx.execute(
        "DELETE FROM telemetry_meta WHERE key IN (?1, ?2)",
        params![TURN_PROGRESS_KEY, EDIT_PROGRESS_KEY],
    )?;
    tx.execute(
        "INSERT INTO telemetry_meta (key, value) VALUES ('schema_version', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [TELEMETRY_SCHEMA_VERSION],
    )?;
    tx.commit()
}

struct ExclusiveTelemetryLock<'conn> {
    conn: &'conn Connection,
    active: bool,
}

impl<'conn> ExclusiveTelemetryLock<'conn> {
    fn acquire(conn: &'conn Connection) -> rusqlite::Result<Self> {
        conn.busy_timeout(TELEMETRY_LOCK_TIMEOUT)?;
        conn.pragma_update(None, "locking_mode", "EXCLUSIVE")?;
        Ok(Self { conn, active: true })
    }

    fn release(mut self) -> rusqlite::Result<()> {
        let result = self.conn.pragma_update(None, "locking_mode", "NORMAL");
        if result.is_ok() {
            self.active = false;
        }
        result
    }
}

impl Drop for ExclusiveTelemetryLock<'_> {
    fn drop(&mut self) {
        if self.active {
            let _ = self.conn.pragma_update(None, "locking_mode", "NORMAL");
        }
    }
}

fn seed_source_lookups(conn: &Connection) -> rusqlite::Result<()> {
    let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
    tx.execute(
        "INSERT OR IGNORE INTO telemetry_strings(kind, value)
         SELECT 'source_key', source_key FROM telemetry_sources",
        [],
    )?;
    tx.execute(
        "INSERT OR IGNORE INTO telemetry_strings(kind, value)
         SELECT 'session', session_id FROM telemetry_sources",
        [],
    )?;
    tx.execute(
        "INSERT OR IGNORE INTO telemetry_strings(kind, value)
         SELECT 'provider', provider FROM telemetry_sources",
        [],
    )?;
    tx.execute(
        "INSERT OR IGNORE INTO telemetry_source_paths(source_ref, source_path)
         SELECT strings.string_id, sources.source_path
         FROM telemetry_sources sources
         JOIN telemetry_strings strings
           ON strings.kind = 'source_key' AND strings.value = sources.source_key",
        [],
    )?;
    tx.commit()
}

fn copy_fact_table(
    conn: &Connection,
    legacy_table: &str,
    normalized_table: &str,
    progress_key: &str,
    turns: bool,
    batch_hook: &mut Option<&mut dyn FnMut() -> rusqlite::Result<()>>,
) -> rusqlite::Result<()> {
    let mut last_id = progress_value(conn, progress_key)?;
    while let Some(end_id) = next_batch_end(conn, legacy_table, last_id)? {
        let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate)?;
        if turns {
            tx.execute(
                &format!(
                    "INSERT OR IGNORE INTO telemetry_strings(kind, value)
                     SELECT 'session', session_id FROM {legacy_table}
                     WHERE id > ?1 AND id <= ?2
                     UNION
                     SELECT 'provider', provider FROM {legacy_table}
                     WHERE id > ?1 AND id <= ?2
                     UNION
                     SELECT 'model', model FROM {legacy_table}
                     WHERE id > ?1 AND id <= ?2 AND model IS NOT NULL
                     UNION
                     SELECT 'effort', effort FROM {legacy_table}
                     WHERE id > ?1 AND id <= ?2 AND effort IS NOT NULL
                     UNION
                     SELECT 'source_key', source_key FROM {legacy_table}
                     WHERE id > ?1 AND id <= ?2"
                ),
                params![last_id, end_id],
            )?;
            tx.execute(
                &format!(
                    "INSERT OR IGNORE INTO telemetry_source_paths(source_ref, source_path)
                     SELECT strings.string_id, facts.source_path
                     FROM {legacy_table} facts
                     JOIN telemetry_strings strings
                       ON strings.kind = 'source_key' AND strings.value = facts.source_key
                     WHERE facts.id > ?1 AND facts.id <= ?2
                     GROUP BY facts.source_key"
                ),
                params![last_id, end_id],
            )?;
            tx.execute(
                &format!(
                    "INSERT OR IGNORE INTO {normalized_table}(
                         id, event_key, session_ref, provider_ref, turn_id, model_ref,
                         effort_ref, started_at, ended_at, input_tokens,
                         cached_input_tokens, cache_write_tokens, output_tokens,
                         reasoning_tokens, context_window, cost_usd, source_ref
                     )
                     SELECT facts.id, facts.event_key, session.string_id,
                            provider.string_id, facts.turn_id, model.string_id,
                            effort.string_id, facts.started_at, facts.ended_at,
                            facts.input_tokens, facts.cached_input_tokens,
                            facts.cache_write_tokens, facts.output_tokens,
                            facts.reasoning_tokens, facts.context_window,
                            facts.cost_usd, source.string_id
                     FROM {legacy_table} facts
                     JOIN telemetry_strings session
                       ON session.kind = 'session' AND session.value = facts.session_id
                     JOIN telemetry_strings provider
                       ON provider.kind = 'provider' AND provider.value = facts.provider
                     LEFT JOIN telemetry_strings model
                       ON model.kind = 'model' AND model.value = facts.model
                     LEFT JOIN telemetry_strings effort
                       ON effort.kind = 'effort' AND effort.value = facts.effort
                     JOIN telemetry_strings source
                       ON source.kind = 'source_key' AND source.value = facts.source_key
                     WHERE facts.id > ?1 AND facts.id <= ?2"
                ),
                params![last_id, end_id],
            )?;
        } else {
            tx.execute(
                &format!(
                    "INSERT OR IGNORE INTO telemetry_strings(kind, value)
                     SELECT 'session', session_id FROM {legacy_table}
                     WHERE id > ?1 AND id <= ?2
                     UNION
                     SELECT 'provider', provider FROM {legacy_table}
                     WHERE id > ?1 AND id <= ?2
                     UNION
                     SELECT 'source_key', source_key FROM {legacy_table}
                     WHERE id > ?1 AND id <= ?2"
                ),
                params![last_id, end_id],
            )?;
            tx.execute(
                &format!(
                    "INSERT OR IGNORE INTO telemetry_source_paths(source_ref, source_path)
                     SELECT strings.string_id, facts.source_path
                     FROM {legacy_table} facts
                     JOIN telemetry_strings strings
                       ON strings.kind = 'source_key' AND strings.value = facts.source_key
                     WHERE facts.id > ?1 AND facts.id <= ?2
                     GROUP BY facts.source_key"
                ),
                params![last_id, end_id],
            )?;
            tx.execute(
                &format!(
                    "INSERT OR IGNORE INTO {normalized_table}(
                         id, event_key, session_ref, provider_ref, turn_id, occurred_at,
                         workspace, path, op, lines_added, lines_removed, source_ref
                     )
                     SELECT facts.id, facts.event_key, session.string_id,
                            provider.string_id, facts.turn_id, facts.occurred_at,
                            facts.workspace, facts.path, facts.op, facts.lines_added,
                            facts.lines_removed, source.string_id
                     FROM {legacy_table} facts
                     JOIN telemetry_strings session
                       ON session.kind = 'session' AND session.value = facts.session_id
                     JOIN telemetry_strings provider
                       ON provider.kind = 'provider' AND provider.value = facts.provider
                     JOIN telemetry_strings source
                       ON source.kind = 'source_key' AND source.value = facts.source_key
                     WHERE facts.id > ?1 AND facts.id <= ?2"
                ),
                params![last_id, end_id],
            )?;
        }
        tx.execute(
            "INSERT INTO telemetry_meta (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![progress_key, end_id],
        )?;
        tx.commit()?;
        if let Some(hook) = batch_hook.as_deref_mut() {
            hook()?;
        }
        last_id = end_id;
    }
    Ok(())
}

fn progress_value(conn: &Connection, key: &str) -> rusqlite::Result<i64> {
    Ok(conn
        .query_row(
            "SELECT value FROM telemetry_meta WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or(0))
}

fn next_batch_end(conn: &Connection, table: &str, last_id: i64) -> rusqlite::Result<Option<i64>> {
    let offset = COPY_BATCH_SIZE - 1;
    let nth = conn
        .query_row(
            &format!("SELECT id FROM {table} WHERE id > ?1 ORDER BY id LIMIT 1 OFFSET {offset}"),
            params![last_id],
            |row| row.get(0),
        )
        .optional()?;
    if nth.is_some() {
        return Ok(nth);
    }
    conn.query_row(
        &format!("SELECT MAX(id) FROM {table} WHERE id > ?1"),
        params![last_id],
        |row| row.get(0),
    )
}

fn verify_migrated_counts(conn: &Connection) -> rusqlite::Result<()> {
    for (legacy, normalized) in [
        ("telemetry_turns", "telemetry_turn_facts"),
        ("telemetry_edits", "telemetry_edit_facts"),
    ] {
        let legacy_count: i64 =
            conn.query_row(&format!("SELECT count(*) FROM {legacy}"), [], |row| {
                row.get(0)
            })?;
        let normalized_count: i64 =
            conn.query_row(&format!("SELECT count(*) FROM {normalized}"), [], |row| {
                row.get(0)
            })?;
        if legacy_count != normalized_count {
            return Err(rusqlite::Error::ToSqlConversionFailure(Box::new(
                std::io::Error::other(format!(
                    "telemetry migration count mismatch for {legacy}: {legacy_count} != {normalized_count}"
                )),
            )));
        }
    }
    Ok(())
}

fn reset_telemetry_schema(conn: &Connection) -> rusqlite::Result<()> {
    for name in TELEMETRY_TABLES.into_iter().chain(NORMALIZED_TABLES) {
        drop_object(conn, name)?;
    }
    Ok(())
}

fn drop_object(conn: &Connection, name: &str) -> rusqlite::Result<()> {
    match object_kind(conn, name)?.as_deref() {
        Some("table") => {
            conn.execute(&format!("DROP TABLE {name}"), [])?;
        }
        Some("view") => {
            conn.execute(&format!("DROP VIEW {name}"), [])?;
        }
        _ => {}
    }
    Ok(())
}

fn object_kind(conn: &Connection, name: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT type FROM sqlite_master WHERE name = ?1",
        params![name],
        |row| row.get(0),
    )
    .optional()
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

    fn create_legacy_fixture(conn: &Connection, rows: usize) {
        conn.execute_batch(
            "CREATE TABLE telemetry_meta (key TEXT PRIMARY KEY, value INTEGER NOT NULL);
             INSERT INTO telemetry_meta(key, value) VALUES ('schema_version', 4);
             CREATE TABLE telemetry_sources (
                 source_key TEXT PRIMARY KEY,
                 source_path TEXT NOT NULL,
                 session_id TEXT NOT NULL,
                 provider TEXT NOT NULL
             );
             CREATE TABLE telemetry_turns (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 event_key TEXT NOT NULL,
                 session_id TEXT NOT NULL,
                 provider TEXT NOT NULL,
                 turn_id TEXT,
                 model TEXT,
                 effort TEXT,
                 started_at TEXT,
                 ended_at TEXT NOT NULL,
                 input_tokens INTEGER,
                 cached_input_tokens INTEGER,
                 cache_write_tokens INTEGER,
                 output_tokens INTEGER,
                 reasoning_tokens INTEGER,
                 context_window INTEGER,
                 cost_usd REAL,
                 source_key TEXT NOT NULL,
                 source_path TEXT NOT NULL
             );
             CREATE TABLE telemetry_edits (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 event_key TEXT NOT NULL,
                 session_id TEXT NOT NULL,
                 provider TEXT NOT NULL,
                 turn_id TEXT,
                 occurred_at TEXT NOT NULL,
                 workspace TEXT,
                 path TEXT NOT NULL,
                 op TEXT NOT NULL,
                 lines_added INTEGER,
                 lines_removed INTEGER,
                 source_key TEXT NOT NULL,
                 source_path TEXT NOT NULL
             );
             INSERT INTO telemetry_sources(source_key, source_path, session_id, provider)
                 VALUES ('source-a', 'session.log', 'session-a', 'codex');",
        )
        .unwrap();

        let tx = Transaction::new_unchecked(conn, TransactionBehavior::Immediate).unwrap();
        for index in 0..rows {
            let timestamp = format!("2026-08-30T{:02}:00:00.000Z", index % 24);
            tx.execute(
                "INSERT INTO telemetry_turns
                    (event_key, session_id, provider, turn_id, model, effort,
                     started_at, ended_at, input_tokens, output_tokens, cost_usd,
                     source_key, source_path)
                 VALUES (?1, 'session-a', 'codex', ?2, 'model-a', 'high',
                         ?3, ?3, ?4, ?5, 0.25, 'source-a', 'session.log')",
                params![
                    format!("event-{index}"),
                    format!("turn-{index}"),
                    timestamp,
                    index as i64,
                    (index * 2) as i64,
                ],
            )
            .unwrap();
            tx.execute(
                "INSERT INTO telemetry_edits
                    (event_key, session_id, provider, turn_id, occurred_at, workspace,
                     path, op, lines_added, lines_removed, source_key, source_path)
                 VALUES (?1, 'session-a', 'codex', ?2, ?3, 'workspace-a',
                         ?4, 'modify', 4, 1, 'source-a', 'session.log')",
                params![
                    format!("edit-{index}"),
                    format!("turn-{index}"),
                    timestamp,
                    format!("src/{index}.rs"),
                ],
            )
            .unwrap();
        }
        tx.commit().unwrap();
    }

    #[test]
    fn migrations_create_every_table() {
        let conn = Connection::open_in_memory().unwrap();
        run_telemetry_migrations(&conn).unwrap();
        assert_eq!(
            table_names(&conn),
            vec![
                "telemetry_activity",
                "telemetry_edit_facts",
                "telemetry_limits",
                "telemetry_meta",
                "telemetry_rollup_hourly",
                "telemetry_source_paths",
                "telemetry_sources",
                "telemetry_strings",
                "telemetry_turn_facts",
            ]
        );
    }

    #[test]
    fn migrations_are_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        run_telemetry_migrations(&conn).unwrap();
        run_telemetry_migrations(&conn).unwrap();
        assert_eq!(table_names(&conn).len(), 9);
    }

    #[test]
    fn legacy_normalization_preserves_values_and_resumes_from_progress() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE telemetry_meta (key TEXT PRIMARY KEY, value INTEGER NOT NULL);
             INSERT INTO telemetry_meta(key, value) VALUES ('schema_version', 4);
             CREATE TABLE telemetry_sources (
                 source_key TEXT PRIMARY KEY,
                 source_path TEXT NOT NULL,
                 session_id TEXT NOT NULL,
                 provider TEXT NOT NULL
             );
             CREATE TABLE telemetry_turns (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 event_key TEXT NOT NULL,
                 session_id TEXT NOT NULL,
                 provider TEXT NOT NULL,
                 turn_id TEXT,
                 model TEXT,
                 effort TEXT,
                 started_at TEXT,
                 ended_at TEXT NOT NULL,
                 input_tokens INTEGER,
                 cached_input_tokens INTEGER,
                 cache_write_tokens INTEGER,
                 output_tokens INTEGER,
                 reasoning_tokens INTEGER,
                 context_window INTEGER,
                 cost_usd REAL,
                 source_key TEXT NOT NULL,
                 source_path TEXT NOT NULL
             );
             CREATE TABLE telemetry_edits (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 event_key TEXT NOT NULL,
                 session_id TEXT NOT NULL,
                 provider TEXT NOT NULL,
                 turn_id TEXT,
                 occurred_at TEXT NOT NULL,
                 workspace TEXT,
                 path TEXT NOT NULL,
                 op TEXT NOT NULL,
                 lines_added INTEGER,
                 lines_removed INTEGER,
                 source_key TEXT NOT NULL,
                 source_path TEXT NOT NULL
             );
             INSERT INTO telemetry_sources(source_key, source_path, session_id, provider)
                 VALUES ('source-a', 'session.log', 'session-a', 'codex');
             INSERT INTO telemetry_turns
                 (event_key, session_id, provider, turn_id, model, effort, started_at,
                  ended_at, input_tokens, output_tokens, cost_usd, source_key, source_path)
                 VALUES
                 ('event-a', 'session-a', 'codex', NULL, 'model-a', 'high',
                  '2026-08-30T00:00:00Z', '2026-08-30T00:01:00Z', 11, 7, 0.25,
                  'source-a', 'session.log'),
                 ('event-b', 'session-a', 'codex', 'turn-b', 'model-a', NULL,
                  '2026-08-30T00:02:00Z', '2026-08-30T00:03:00Z', 13, 9, 0.5,
                  'source-a', 'session.log');
             INSERT INTO telemetry_edits
                 (event_key, session_id, provider, turn_id, occurred_at, workspace, path,
                  op, lines_added, lines_removed, source_key, source_path)
                 VALUES ('edit-a', 'session-a', 'codex', 'turn-b',
                         '2026-08-30T00:03:00Z', 'workspace-a', 'src/lib.rs',
                         'modify', 4, 1, 'source-a', 'session.log');",
        )
        .unwrap();

        // Model the process stopping after committing the first turn batch:
        // the normalized row and its progress marker are durable, while the
        // legacy tables remain available for the next invocation to resume.
        create_normalized_schema(&conn).unwrap();
        let session_ref = ensure_string_id(&conn, STRING_SESSION, "session-a").unwrap();
        let provider_ref = ensure_string_id(&conn, STRING_PROVIDER, "codex").unwrap();
        let model_ref = ensure_string_id(&conn, STRING_MODEL, "model-a").unwrap();
        let effort_ref = ensure_string_id(&conn, STRING_EFFORT, "high").unwrap();
        let source_ref = ensure_source_ref(&conn, "source-a", "session.log").unwrap();
        conn.execute(
            "INSERT INTO telemetry_turn_facts
                 (id, event_key, session_ref, provider_ref, model_ref, effort_ref,
                  started_at, ended_at, input_tokens, output_tokens, cost_usd, source_ref)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            rusqlite::params![
                1,
                "event-a",
                session_ref,
                provider_ref,
                model_ref,
                effort_ref,
                "2026-08-30T00:00:00Z",
                "2026-08-30T00:01:00Z",
                11,
                7,
                0.25,
                source_ref,
            ],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO telemetry_meta(key, value)
             VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            rusqlite::params![TURN_PROGRESS_KEY, 1],
        )
        .unwrap();

        run_telemetry_migrations(&conn).unwrap();

        let turn_count: i64 = conn
            .query_row("SELECT count(*) FROM telemetry_turns", [], |row| row.get(0))
            .unwrap();
        let edit_count: i64 = conn
            .query_row("SELECT count(*) FROM telemetry_edits", [], |row| row.get(0))
            .unwrap();
        assert_eq!(turn_count, 2);
        assert_eq!(edit_count, 1);
        let (event_key, input_tokens, model, effort): (String, i64, String, Option<String>) = conn
            .query_row(
                "SELECT event_key, input_tokens, model, effort
                 FROM telemetry_turns WHERE event_key = 'event-b'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(event_key, "event-b");
        assert_eq!(input_tokens, 13);
        assert_eq!(model, "model-a");
        assert_eq!(effort, None);
        assert_eq!(
            conn.query_row::<i64, _, _>(
                "SELECT count(*) FROM telemetry_meta
                 WHERE key IN ('normalization_turn_last_id', 'normalization_edit_last_id')",
                [],
                |row| row.get(0),
            )
            .unwrap(),
            0
        );
        assert_eq!(
            conn.query_row::<i64, _, _>(
                "SELECT value FROM telemetry_meta WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .unwrap(),
            TELEMETRY_SCHEMA_VERSION
        );
    }

    #[test]
    fn legacy_normalization_reopens_after_a_committed_batch_failure() {
        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("state.db");
        {
            let conn = Connection::open(&database).unwrap();
            create_legacy_fixture(&conn, COPY_BATCH_SIZE as usize * 2 + 1);

            let mut batches = 0;
            let mut fail_after_first = || {
                batches += 1;
                if batches == 1 {
                    Err(sqlite_io_error(io::Error::other(
                        "injected interruption after committed batch",
                    )))
                } else {
                    Ok(())
                }
            };
            let mut hook: Option<&mut dyn FnMut() -> rusqlite::Result<()>> =
                Some(&mut fail_after_first);
            assert!(migrate_legacy_schema_with_hook(&conn, &mut hook).is_err());
            assert_eq!(
                conn.query_row::<i64, _, _>(
                    "SELECT value FROM telemetry_meta WHERE key = ?1",
                    params![TURN_PROGRESS_KEY],
                    |row| row.get(0),
                )
                .unwrap(),
                COPY_BATCH_SIZE
            );
            assert_eq!(
                conn.query_row::<i64, _, _>(
                    "SELECT count(*) FROM telemetry_turn_facts",
                    [],
                    |row| row.get(0),
                )
                .unwrap(),
                COPY_BATCH_SIZE
            );
        }

        let conn = Connection::open(&database).unwrap();
        run_telemetry_migrations(&conn).unwrap();
        let expected = COPY_BATCH_SIZE * 2 + 1;
        assert_eq!(
            conn.query_row::<i64, _, _>("SELECT count(*) FROM telemetry_turns", [], |row| {
                row.get(0)
            })
            .unwrap(),
            expected
        );
        assert_eq!(
            conn.query_row::<i64, _, _>("SELECT count(*) FROM telemetry_edits", [], |row| {
                row.get(0)
            })
            .unwrap(),
            expected
        );
        assert_eq!(
            conn.query_row::<String, _, _>(
                "SELECT model FROM telemetry_turns WHERE event_key = 'event-4000'",
                [],
                |row| row.get(0),
            )
            .unwrap(),
            "model-a"
        );
        assert_eq!(
            conn.query_row::<i64, _, _>(
                "SELECT count(*) FROM telemetry_meta
                 WHERE key IN ('normalization_turn_last_id', 'normalization_edit_last_id')",
                [],
                |row| row.get(0),
            )
            .unwrap(),
            0
        );
    }

    #[test]
    fn concurrent_file_migrations_share_one_lease() {
        use std::sync::{Arc, Barrier};
        use std::thread;

        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("state.db");
        create_legacy_fixture(&Connection::open(&database).unwrap(), 32);

        let barrier = Arc::new(Barrier::new(2));
        let handles = (0..2)
            .map(|_| {
                let database = database.clone();
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    let conn = Connection::open(database)?;
                    barrier.wait();
                    run_telemetry_migrations(&conn)
                })
            })
            .collect::<Vec<_>>();
        for handle in handles {
            handle.join().unwrap().unwrap();
        }

        let conn = Connection::open(&database).unwrap();
        assert_eq!(
            conn.query_row::<i64, _, _>("SELECT count(*) FROM telemetry_turns", [], |row| {
                row.get(0)
            })
            .unwrap(),
            32
        );
        assert_eq!(
            conn.query_row::<i64, _, _>("SELECT count(*) FROM telemetry_edits", [], |row| {
                row.get(0)
            })
            .unwrap(),
            32
        );
    }

    #[test]
    fn legacy_writer_is_fenced_between_committed_migration_batches() {
        use rusqlite::OpenFlags;
        use std::sync::{Arc, Mutex};

        let directory = tempfile::tempdir().unwrap();
        let database = directory.path().join("state.db");
        create_legacy_fixture(
            &Connection::open(&database).unwrap(),
            COPY_BATCH_SIZE as usize * 2 + 1,
        );

        let writer_was_blocked = Arc::new(Mutex::new(false));
        let database_for_hook = database.clone();
        let writer_was_blocked_for_hook = Arc::clone(&writer_was_blocked);
        let mut attempted = false;
        let mut legacy_writer = move || {
            if attempted {
                return Ok(());
            }
            attempted = true;
            let writer =
                Connection::open_with_flags(&database_for_hook, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
            writer.busy_timeout(Duration::from_millis(50))?;
            let result = writer.execute(
                "INSERT INTO telemetry_turns
                    (event_key, session_id, provider, ended_at, source_key, source_path)
                 VALUES ('legacy-race', 'session-a', 'codex',
                         '2026-08-30T00:00:00.000Z', 'source-a', 'session.log')",
                [],
            );
            *writer_was_blocked_for_hook.lock().unwrap() = result.is_err();
            Ok(())
        };
        let mut hook: Option<&mut dyn FnMut() -> rusqlite::Result<()>> = Some(&mut legacy_writer);
        let conn = Connection::open(&database).unwrap();
        migrate_legacy_schema_with_hook(&conn, &mut hook).unwrap();

        assert!(*writer_was_blocked.lock().unwrap());
        assert_eq!(
            conn.query_row::<i64, _, _>("SELECT count(*) FROM telemetry_turns", [], |row| {
                row.get(0)
            })
            .unwrap(),
            COPY_BATCH_SIZE * 2 + 1
        );
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
    fn duplicate_turn_is_ignored() {
        let conn = Connection::open_in_memory().unwrap();
        run_telemetry_migrations(&conn).unwrap();
        let insert = "INSERT INTO telemetry_turns (event_key, session_id, provider, turn_id, ended_at, source_key, source_path)
                      VALUES ('e1', 's', 'codex', 't1', '2026-08-13T00:00:00Z', 'sk', 'log.jsonl')";
        conn.execute(insert, []).unwrap();
        // Re-ingest of the same delta must not double count.
        conn.execute(insert, []).unwrap();
        let count: i64 = conn
            .query_row("SELECT count(*) FROM telemetry_turn_facts", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
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
        conn.execute(insert, []).unwrap();
        let count: i64 = conn
            .query_row("SELECT count(*) FROM telemetry_turn_facts", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
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
        conn.execute(
            "INSERT INTO telemetry_turns (event_key, session_id, provider, turn_id, ended_at, source_key, source_path)
             VALUES ('e1', 's', 'codex', 't1', '2026-08-13T00:00:00Z', 'sk', 'log.jsonl')",
            [],
        )
        .unwrap();
        let count: i64 = conn
            .query_row("SELECT count(*) FROM telemetry_turn_facts", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
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
