//! Phase 1 correctness gate.
//!
//! OpenCode is the only provider that publishes its own aggregates alongside
//! the per-turn rows they came from: the `session` table carries `tokens_*` and
//! `summary_*` columns summarizing the same `message` and `part` rows this
//! crate ingests. That makes it the one true end-to-end check available —
//! everywhere else we would only be comparing our arithmetic against itself.
//!
//! These tests build a fixture whose session totals are consistent with its
//! messages, ingest it, and require the store to arrive at the same numbers.

use rusqlite::{params, Connection};
use wardian_core::telemetry::ingest::ingest_source;
use wardian_core::telemetry::query::{summary, Dimension};
use wardian_core::telemetry::schema::run_telemetry_migrations;
use wardian_core::telemetry::sources::SourceContext;

const SESSION: &str = "ses_recon";
const FROM: &str = "2026-08-13T00:00:00.000Z";
const TO: &str = "2026-08-14T00:00:00.000Z";

/// Per-turn figures, mirroring the shape opencode stores in `message.data`.
struct Turn {
    id: &'static str,
    created: i64,
    completed: i64,
    input: i64,
    output: i64,
    reasoning: i64,
    cache_read: i64,
    cache_write: i64,
    cost: f64,
}

const TURNS: &[Turn] = &[
    Turn {
        id: "msg_1",
        created: 1786644338919,
        completed: 1786644345306,
        input: 10639,
        output: 68,
        reasoning: 0,
        cache_read: 1792,
        cache_write: 0,
        cost: 0.0125,
    },
    Turn {
        id: "msg_2",
        created: 1786644400000,
        completed: 1786644460000,
        input: 6545,
        output: 57,
        reasoning: 12,
        cache_read: 5888,
        cache_write: 128,
        cost: 0.0075,
    },
    Turn {
        id: "msg_3",
        created: 1786644500000,
        completed: 1786644530000,
        input: 10633,
        output: 65,
        reasoning: 4,
        cache_read: 1792,
        cache_write: 0,
        cost: 0.011,
    },
];

/// Build a database with opencode's schema for the columns we read, including
/// the provider's own pre-aggregated session totals.
fn opencode_fixture(path: &std::path::Path) {
    let conn = Connection::open(path).unwrap();
    conn.execute_batch(
        "CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER,
                               time_updated INTEGER, data TEXT);
         CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT, session_id TEXT,
                            time_created INTEGER, time_updated INTEGER, data TEXT);
         CREATE TABLE session (id TEXT PRIMARY KEY, directory TEXT, agent TEXT, model TEXT,
                               cost REAL, tokens_input INTEGER, tokens_output INTEGER,
                               tokens_reasoning INTEGER, tokens_cache_read INTEGER,
                               tokens_cache_write INTEGER, summary_additions INTEGER,
                               summary_deletions INTEGER, summary_files INTEGER,
                               time_created INTEGER, time_updated INTEGER);",
    )
    .unwrap();

    for turn in TURNS {
        let data = format!(
            r#"{{"role":"assistant","mode":"build","agent":"build",
                 "path":{{"cwd":"D:\\repo","root":"D:\\repo"}},"cost":{},
                 "tokens":{{"input":{},"output":{},"reasoning":{},
                            "cache":{{"read":{},"write":{}}}}},
                 "modelID":"deepseek-v4-flash-free","providerID":"opencode",
                 "time":{{"created":{},"completed":{}}},"finish":"stop"}}"#,
            turn.cost,
            turn.input,
            turn.output,
            turn.reasoning,
            turn.cache_read,
            turn.cache_write,
            turn.created,
            turn.completed
        );
        conn.execute(
            "INSERT INTO message (id, session_id, time_created, time_updated, data)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![turn.id, SESSION, turn.created, turn.completed, data],
        )
        .unwrap();
    }

    // Two files touched: one created, one edited twice.
    let parts = [
        (
            "prt_1",
            1786644345000_i64,
            r#"{"type":"tool","tool":"write","state":{"status":"completed",
                "input":{"filePath":"D:/repo/new.md","content":"a\nb\nc"}}}"#,
        ),
        (
            "prt_2",
            1786644455000,
            r#"{"type":"tool","tool":"edit","state":{"status":"completed",
                "input":{"filePath":"D:/repo/a.md","oldString":"x\nOLD\ny","newString":"x\nNEW\ny"}}}"#,
        ),
        (
            "prt_3",
            1786644525000,
            r#"{"type":"tool","tool":"edit","state":{"status":"completed",
                "input":{"filePath":"D:/repo/a.md","oldString":"p\nq","newString":"p\nq\nr"}}}"#,
        ),
    ];
    for (id, updated, data) in parts {
        conn.execute(
            "INSERT INTO part (id, message_id, session_id, time_created, time_updated, data)
             VALUES (?1, 'msg_1', ?2, ?3, ?3, ?4)",
            params![id, SESSION, updated, data],
        )
        .unwrap();
    }

    // The provider's own aggregates, written as **literals** rather than as
    // sums over `TURNS`.
    //
    // This is the whole point of the reconciliation: the test claims ingest
    // reproduces figures opencode arrived at independently. Computing them here
    // with the same expression the assertion later compares against would make
    // both sides move together, so a wrong assumption about the provider's
    // token semantics would produce a passing test. That is not hypothetical —
    // it is exactly how a 49x token overstatement survived phase 1, in a fixture
    // whose numbers were derived from the values they were meant to check.
    //
    // Transcribed by hand from the message rows above:
    //   input      10639 + 6545 + 10633 = 27817   (cache-exclusive, per opencode)
    //   output        68 +   57 +    65 =   190
    //   reasoning      0 +   12 +     4 =    16
    //   cache_read  1792 + 5888 +  1792 =  9472
    //   cache_write    0 +  128 +     0 =    128
    //   cost      0.0125 + 0.0075 + 0.011 = 0.031
    //   additions 3 (write) + 1 (edit) + 1 (edit) = 5
    //   deletions 0 + 1 + 0 = 1;  files = 2 distinct paths
    //
    // A change to `TURNS` must therefore break this test until these are
    // updated to match, which is the failure the reconciliation exists to give.
    conn.execute(
        "INSERT INTO session (id, directory, agent, model, cost, tokens_input, tokens_output,
                              tokens_reasoning, tokens_cache_read, tokens_cache_write,
                              summary_additions, summary_deletions, summary_files,
                              time_created, time_updated)
         VALUES (?1, 'D:/repo', 'build', '{\"id\":\"deepseek-v4-flash-free\"}', 0.031,
                 27817, 190, 16, 9472, 128, 5, 1, 2, ?2, ?3)",
        params![SESSION, TURNS[0].created, TURNS[TURNS.len() - 1].completed],
    )
    .unwrap();
}

/// Add a second session's rows to an existing fixture database.
///
/// OpenCode keeps one database per machine, so this is the normal case rather
/// than an exotic one: every agent using opencode writes into the same file.
fn add_second_session(conn: &Connection, session: &str, base_time: i64, input: i64) {
    add_session_message(conn, session, base_time, input, 0)
}

/// One assistant message, with an explicit cache-read figure.
fn add_session_message(
    conn: &Connection,
    session: &str,
    base_time: i64,
    input: i64,
    cache_read: i64,
) {
    let data = format!(
        r#"{{"role":"assistant","tokens":{{"input":{input},"output":7,"reasoning":0,
             "cache":{{"read":{cache_read},"write":0}}}},"modelID":"m","providerID":"opencode",
             "time":{{"created":{base_time},"completed":{}}},"finish":"stop"}}"#,
        base_time + 1_000
    );
    conn.execute(
        "INSERT INTO message (id, session_id, time_created, time_updated, data)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            format!("msg_{session}"),
            session,
            base_time,
            base_time + 1_000,
            data
        ],
    )
    .unwrap();
}

struct Harness {
    _dir: tempfile::TempDir,
    store: Connection,
    provider_db: std::path::PathBuf,
}

fn ingest_fixture() -> Harness {
    let dir = tempfile::tempdir().unwrap();
    let provider_db = dir.path().join("opencode.db");
    opencode_fixture(&provider_db);

    let store = Connection::open_in_memory().unwrap();
    run_telemetry_migrations(&store).unwrap();

    let ctx = SourceContext::new("agent-1", "opencode", &provider_db)
        .with_provider_session_id(Some(SESSION.to_string()));
    ingest_source(&store, &ctx).unwrap();

    Harness {
        _dir: dir,
        store,
        provider_db,
    }
}

/// The provider's own session-level aggregates.
fn provider_totals(path: &std::path::Path) -> (i64, i64, i64, i64, i64, i64, i64, i64) {
    let conn = Connection::open(path).unwrap();
    conn.query_row(
        "SELECT tokens_input, tokens_output, tokens_reasoning, tokens_cache_read,
                tokens_cache_write, summary_additions, summary_deletions, summary_files
         FROM session WHERE id = ?1",
        params![SESSION],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
            ))
        },
    )
    .unwrap()
}

#[test]
fn ingested_tokens_reconcile_with_the_providers_own_session_totals() {
    let harness = ingest_fixture();
    let (input, output, reasoning, cache_read, cache_write, ..) =
        provider_totals(&harness.provider_db);
    let summary = summary(&harness.store, FROM, TO).unwrap();

    assert_eq!(summary.tokens.input_tokens.unwrap(), input);
    assert_eq!(summary.tokens.output_tokens.unwrap(), output);
    assert_eq!(summary.tokens.reasoning_tokens.unwrap(), reasoning);
    assert_eq!(summary.tokens.cached_input_tokens.unwrap(), cache_read);
    assert_eq!(summary.tokens.cache_write_tokens.unwrap(), cache_write);
}

#[test]
fn ingested_edits_reconcile_with_the_providers_own_summary() {
    let harness = ingest_fixture();
    let (.., additions, deletions, files) = provider_totals(&harness.provider_db);
    let summary = summary(&harness.store, FROM, TO).unwrap();

    assert_eq!(summary.lines_added, additions);
    assert_eq!(summary.lines_removed, deletions);
    assert_eq!(summary.files_touched, files);
}

#[test]
fn every_turn_is_ingested_exactly_once() {
    let harness = ingest_fixture();
    let summary = summary(&harness.store, FROM, TO).unwrap();
    assert_eq!(summary.turns, TURNS.len() as i64);
}

#[test]
fn cache_reads_are_never_folded_into_fresh_input() {
    let harness = ingest_fixture();
    let summary = summary(&harness.store, FROM, TO).unwrap();
    let (input, _, _, cache_read, ..) = provider_totals(&harness.provider_db);

    // Both are real and both are large; conflating them is the failure this
    // guards against.
    assert!(cache_read > 0);
    assert_eq!(summary.tokens.input_tokens.unwrap(), input);
    assert_ne!(summary.tokens.input_tokens.unwrap(), input + cache_read);
}

#[test]
fn a_cache_dominated_opencode_session_keeps_its_fresh_input_intact() {
    // The asymmetry that makes `input_tokens` a normalized field, checked where
    // getting it wrong would be worst.
    //
    // Codex reports a prompt total *including* cache reads and its source
    // subtracts them. OpenCode reports the two disjointly, so it must subtract
    // nothing. Applying codex's rule here would look entirely plausible — the
    // figure would just be smaller — so this pins the case where the mistake is
    // largest: 400 fresh tokens against 96,000 cache reads, the ~99% hit rate a
    // long conversation actually reaches.
    //
    // A wrongly-applied subtraction would report 0 fresh input (clamped) and
    // read as "this agent processed nothing new", which is a claim about the
    // agent rather than an arithmetic slip.
    let dir = tempfile::tempdir().unwrap();
    let provider_db = dir.path().join("opencode.db");
    opencode_fixture(&provider_db);

    let session = "ses_cache_heavy";
    {
        let conn = Connection::open(&provider_db).unwrap();
        add_session_message(&conn, session, 1_786_644_600_000, 400, 96_000);
    }

    let store = Connection::open_in_memory().unwrap();
    run_telemetry_migrations(&store).unwrap();
    let ctx = SourceContext::new("agent-cache", "opencode", &provider_db)
        .with_provider_session_id(Some(session.to_string()));
    ingest_source(&store, &ctx).unwrap();

    let (input, cached): (i64, i64) = store
        .query_row(
            "SELECT COALESCE(SUM(input_tokens),0), COALESCE(SUM(cached_input_tokens),0)
             FROM telemetry_turns WHERE session_id = 'agent-cache'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();

    assert_eq!(input, 400, "opencode's input is already cache-exclusive");
    assert_eq!(cached, 96_000);
    // Guard against a vacuous pass: this only proves anything while the cache
    // reads genuinely dominate.
    assert!(cached / input.max(1) > 100);
}

#[test]
fn opencode_active_time_is_measured_not_estimated() {
    let harness = ingest_fixture();
    let summary = summary(&harness.store, FROM, TO).unwrap();

    // Literal, for the same reason the session aggregates are: 6387 + 60000 +
    // 30000, transcribed from the message rows rather than re-derived from them.
    let expected: i64 = 96_387;
    assert_eq!(summary.active.measured_ms, expected);
    assert_eq!(summary.active.clustered_ms, 0);
    // Only one method contributed, so this total is not a mixture and can be
    // presented as a measurement without qualification.
    assert!(!summary.active.is_mixed());
}

#[test]
fn an_agent_ingesting_second_still_sees_its_own_older_history() {
    // OpenCode keeps one database for every agent on the machine. Keyed by path
    // alone, the first agent to ingest left its high-water mark as *the* cursor,
    // and the second resumed from a position inside someone else's history —
    // skipping its own past permanently, because the cursor only moves forward.
    let dir = tempfile::tempdir().unwrap();
    let provider_db = dir.path().join("opencode.db");
    opencode_fixture(&provider_db);

    // Agent B's work is older than agent A's, which is what makes A's cursor
    // dangerous to inherit.
    add_second_session(
        &Connection::open(&provider_db).unwrap(),
        "ses_b",
        1_786_600_000_000,
        4_321,
    );

    let store = Connection::open_in_memory().unwrap();
    run_telemetry_migrations(&store).unwrap();

    let ctx_a = SourceContext::new("agent-a", "opencode", &provider_db)
        .with_provider_session_id(Some(SESSION.to_string()));
    ingest_source(&store, &ctx_a).unwrap();

    let ctx_b = SourceContext::new("agent-b", "opencode", &provider_db)
        .with_provider_session_id(Some("ses_b".to_string()));
    let outcome = ingest_source(&store, &ctx_b).unwrap();

    assert_eq!(outcome.turns, 1, "agent B's own turn must not be skipped");

    let b_input: Option<i64> = store
        .query_row(
            "SELECT SUM(input_tokens) FROM telemetry_turns WHERE session_id = 'agent-b'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(b_input, Some(4_321));

    // And the two agents' facts stay attributed to the agent that produced them.
    let a_turns: i64 = store
        .query_row(
            "SELECT count(*) FROM telemetry_turns WHERE session_id = 'agent-a'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(a_turns, TURNS.len() as i64);
}

#[test]
fn rows_living_only_in_the_write_ahead_log_are_still_read() {
    // The reason `immutable=1` is deliberately not used. It would let SQLite
    // skip the WAL, and since the WAL is exactly where the newest rows live
    // before a checkpoint, the Dashboard would under-report the recent activity
    // it exists to show — silently, and only against a live provider.
    let dir = tempfile::tempdir().unwrap();
    let provider_db = dir.path().join("opencode.db");
    opencode_fixture(&provider_db);

    let writer = Connection::open(&provider_db).unwrap();
    let mode: String = writer
        .query_row("PRAGMA journal_mode=WAL", [], |row| row.get(0))
        .unwrap();
    assert_eq!(mode, "wal", "fixture must actually be in WAL mode");

    // Written through the connection that stays open, so nothing checkpoints
    // these rows into the main database file: they live in the -wal only.
    add_second_session(&writer, "ses_wal", 1_786_644_600_000, 999);
    assert!(
        provider_db.with_extension("db-wal").exists(),
        "the write-ahead log should exist while a writer holds the database"
    );

    let store = Connection::open_in_memory().unwrap();
    run_telemetry_migrations(&store).unwrap();
    let ctx = SourceContext::new("agent-wal", "opencode", &provider_db)
        .with_provider_session_id(Some("ses_wal".to_string()));
    let outcome = ingest_source(&store, &ctx).unwrap();

    assert_eq!(outcome.turns, 1, "WAL-resident rows must be visible");
    let input: Option<i64> = store
        .query_row(
            "SELECT SUM(input_tokens) FROM telemetry_turns WHERE session_id = 'agent-wal'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(input, Some(999));

    drop(writer);
}

#[test]
fn a_database_with_a_live_writer_is_still_readable() {
    // A read that failed whenever the provider held the database would make
    // telemetry stop exactly when the agent is busiest.
    let dir = tempfile::tempdir().unwrap();
    let provider_db = dir.path().join("opencode.db");
    opencode_fixture(&provider_db);

    let writer = Connection::open(&provider_db).unwrap();
    writer
        .query_row("PRAGMA journal_mode=WAL", [], |row| row.get::<_, String>(0))
        .unwrap();
    // An open write transaction: the provider is mid-turn.
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();

    let store = Connection::open_in_memory().unwrap();
    run_telemetry_migrations(&store).unwrap();
    let ctx = SourceContext::new("agent-1", "opencode", &provider_db)
        .with_provider_session_id(Some(SESSION.to_string()));

    let outcome = ingest_source(&store, &ctx).unwrap();
    assert_eq!(outcome.turns, TURNS.len());

    writer.execute_batch("ROLLBACK").unwrap();
}

#[test]
fn re_ingesting_the_same_database_changes_nothing() {
    let harness = ingest_fixture();
    let before = summary(&harness.store, FROM, TO).unwrap();

    let ctx = SourceContext::new("agent-1", "opencode", &harness.provider_db)
        .with_provider_session_id(Some(SESSION.to_string()));
    ingest_source(&harness.store, &ctx).unwrap();
    ingest_source(&harness.store, &ctx).unwrap();

    assert_eq!(summary(&harness.store, FROM, TO).unwrap(), before);
}

#[test]
fn incremental_ingest_reaches_the_same_totals_as_a_single_pass() {
    // A source read in pieces must land on the same figures as one read, or
    // the cursor is losing or duplicating rows.
    let dir = tempfile::tempdir().unwrap();
    let provider_db = dir.path().join("opencode.db");
    opencode_fixture(&provider_db);

    let store = Connection::open_in_memory().unwrap();
    run_telemetry_migrations(&store).unwrap();
    let ctx = SourceContext::new("agent-1", "opencode", &provider_db)
        .with_provider_session_id(Some(SESSION.to_string()));

    // Hide the later turns, ingest, then reveal them one at a time.
    let provider = Connection::open(&provider_db).unwrap();
    let hidden: Vec<(String, i64, String)> = {
        let mut stmt = provider
            .prepare("SELECT id, time_updated, data FROM message WHERE id != 'msg_1'")
            .unwrap();
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        rows
    };
    provider
        .execute("DELETE FROM message WHERE id != 'msg_1'", [])
        .unwrap();
    ingest_source(&store, &ctx).unwrap();

    for (id, updated, data) in hidden {
        provider
            .execute(
                "INSERT INTO message (id, session_id, time_created, time_updated, data)
                 VALUES (?1, ?2, ?3, ?3, ?4)",
                params![id, SESSION, updated, data],
            )
            .unwrap();
        ingest_source(&store, &ctx).unwrap();
    }

    let (input, output, ..) = provider_totals(&provider_db);
    let summary = summary(&store, FROM, TO).unwrap();
    assert_eq!(summary.tokens.input_tokens.unwrap(), input);
    assert_eq!(summary.tokens.output_tokens.unwrap(), output);
    assert_eq!(summary.turns, TURNS.len() as i64);
}

#[test]
fn breakdown_attributes_everything_to_opencode() {
    let harness = ingest_fixture();
    let rows = wardian_core::telemetry::query::breakdown(
        &harness.store,
        Dimension::Provider,
        FROM,
        TO,
        10,
    )
    .unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].key, "opencode");
    assert!(rows[0].tokens_reported);
}

#[test]
fn reading_the_provider_database_leaves_it_unmodified() {
    // Ingest must never write to a provider's live database.
    let dir = tempfile::tempdir().unwrap();
    let provider_db = dir.path().join("opencode.db");
    opencode_fixture(&provider_db);
    let before = std::fs::read(&provider_db).unwrap();

    let store = Connection::open_in_memory().unwrap();
    run_telemetry_migrations(&store).unwrap();
    let ctx = SourceContext::new("agent-1", "opencode", &provider_db)
        .with_provider_session_id(Some(SESSION.to_string()));
    ingest_source(&store, &ctx).unwrap();

    assert_eq!(std::fs::read(&provider_db).unwrap(), before);
}
