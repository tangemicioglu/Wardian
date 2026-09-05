//! Explicit, owned provider-log evidence shared by fixtures and the developer example.

use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde_json::Value;
use wardian_core::telemetry::ingest::{ingest_source, IngestError};
use wardian_core::telemetry::schema::run_telemetry_migrations;
use wardian_core::telemetry::sources::SourceContext;

/// Fresh input, cache reads, cache writes, output, and reasoning (inside output).
pub(crate) type Totals = (i64, i64, i64, i64, i64);

#[derive(Debug, thiserror::Error)]
pub(crate) enum VerifyError {
    #[error("log I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid JSON at line {line}: {source}")]
    Json {
        line: usize,
        source: serde_json::Error,
    },
    #[error("{0}")]
    Invalid(&'static str),
    #[error("telemetry database failed: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("telemetry ingestion failed: {0}")]
    Ingest(#[from] IngestError),
}

/// Owns the only file used after capture; never retains or reopens the input path.
pub(crate) struct ProviderLog {
    _dir: tempfile::TempDir,
    path: PathBuf,
    provider: &'static str,
    declared: Totals,
}

impl ProviderLog {
    /// Read an explicit file once and validate every nonblank JSONL record.
    /// Missing, unreadable, empty, malformed, or accounting-free inputs are errors.
    pub(crate) fn capture(provider: &str, input: &Path) -> Result<Self, VerifyError> {
        let provider = match provider {
            "codex" => "codex",
            "pi" => "pi",
            _ => return Err(VerifyError::Invalid("provider must be codex or pi")),
        };
        let text = std::fs::read_to_string(input)?;
        let mut records = Vec::new();
        for (line, text) in text.lines().enumerate() {
            if text.trim().is_empty() {
                continue;
            }
            let record: Value = serde_json::from_str(text).map_err(|source| VerifyError::Json {
                line: line + 1,
                source,
            })?;
            require(record.is_object(), "log records must be JSON objects")?;
            records.push(record);
        }
        require(!records.is_empty(), "log must contain records")?;
        let declared = match provider {
            "codex" => codex_totals(&records)?,
            _ => pi_totals(&records)?,
        };
        require(
            declared.0 > 0,
            "log must declare positive fresh input tokens",
        )?;

        let dir = tempfile::tempdir()?;
        let path = dir.path().join("provider.jsonl");
        // Preserve the captured bytes, including any final line without a newline.
        std::fs::write(&path, text)?;
        Ok(Self {
            _dir: dir,
            path,
            provider,
            declared,
        })
    }

    /// Reconcile accounting, activity, rollups, and forced reparse on one snapshot.
    pub(crate) fn verify(&self) -> Result<Totals, VerifyError> {
        let store = Connection::open_in_memory()?;
        run_telemetry_migrations(&store)?;
        let ctx = SourceContext::new("agent-fixture", self.provider, &self.path);
        ingest_source(&store, &ctx)?;
        let first = stored_totals(&store)?;
        require(
            first == self.declared,
            "ingested tokens disagree with declared accounting",
        )?;
        let counts = usable_facts(&store)?;

        // A parser version bump must reread the same bytes without losing or
        // multiplying facts. Neither this pass nor the first sees the source again.
        store.execute("UPDATE telemetry_sources SET parser_version = 0", [])?;
        ingest_source(&store, &ctx)?;
        require(
            stored_totals(&store)? == first,
            "forced reparse changed token totals",
        )?;
        require(
            usable_facts(&store)? == counts,
            "forced reparse changed fact counts",
        )?;
        Ok(first)
    }
}

fn require(condition: bool, message: &'static str) -> Result<(), VerifyError> {
    if condition {
        Ok(())
    } else {
        Err(VerifyError::Invalid(message))
    }
}

fn token(value: &Value, key: &str) -> Result<i64, VerifyError> {
    value
        .get(key)
        .and_then(Value::as_i64)
        .filter(|n| *n >= 0)
        .ok_or(VerifyError::Invalid(
            "usage requires nonnegative integer token fields",
        ))
}

fn add(left: i64, right: i64) -> Result<i64, VerifyError> {
    left.checked_add(right)
        .ok_or(VerifyError::Invalid("token total overflows i64"))
}

fn codex_totals(records: &[Value]) -> Result<Totals, VerifyError> {
    let mut latest = None;
    for record in records {
        let payload = &record["payload"];
        if payload["type"].as_str() != Some("token_count") {
            continue;
        }
        let total = &payload["info"]["total_token_usage"];
        let input = token(total, "input_tokens")?;
        let cached = token(total, "cached_input_tokens")?;
        require(cached <= input, "codex cache reads exceed prompt input")?;
        latest = Some((
            input - cached,
            cached,
            0,
            token(total, "output_tokens")?,
            token(total, "reasoning_output_tokens")?,
        ));
    }
    latest.ok_or(VerifyError::Invalid(
        "log must carry codex token_count records",
    ))
}

fn pi_totals(records: &[Value]) -> Result<Totals, VerifyError> {
    let mut totals = (0, 0, 0, 0, 0);
    let mut seen = false;
    for record in records {
        if record["type"].as_str() != Some("message")
            || record["message"]["role"].as_str() != Some("assistant")
        {
            continue;
        }
        let usage = &record["message"]["usage"];
        let input = token(usage, "input")?;
        let cached = token(usage, "cacheRead")?;
        let writes = token(usage, "cacheWrite")?;
        let output = token(usage, "output")?;
        require(
            writes == 0,
            "pi nonzero cacheWrite requires confirming totalTokens semantics",
        )?;
        require(
            token(usage, "totalTokens")? == add(add(input, output)?, cached)?,
            "pi input, output, and cacheRead must reconcile to totalTokens",
        )?;
        let reasoning = if usage.get("reasoning").is_some() {
            token(usage, "reasoning")?
        } else {
            0
        };
        totals = (
            add(totals.0, input)?,
            add(totals.1, cached)?,
            add(totals.2, writes)?,
            add(totals.3, output)?,
            add(totals.4, reasoning)?,
        );
        seen = true;
    }
    require(seen, "log must carry pi assistant usage")?;
    Ok(totals)
}

fn stored_totals(store: &Connection) -> Result<Totals, rusqlite::Error> {
    store.query_row(
        "SELECT COALESCE(SUM(input_tokens),0), COALESCE(SUM(cached_input_tokens),0),
                COALESCE(SUM(cache_write_tokens),0), COALESCE(SUM(output_tokens),0),
                COALESCE(SUM(reasoning_tokens),0) FROM telemetry_turns",
        [],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        },
    )
}

fn usable_facts(store: &Connection) -> Result<(i64, i64, i64), VerifyError> {
    let (turns, edits, intervals, inverted, fact_input, rollup_input): (
        i64,
        i64,
        i64,
        i64,
        i64,
        i64,
    ) = store.query_row(
        "SELECT (SELECT COUNT(*) FROM telemetry_turns),
                    (SELECT COUNT(*) FROM telemetry_edits),
                    (SELECT COUNT(*) FROM telemetry_activity),
                    (SELECT COUNT(*) FROM telemetry_activity WHERE ended_at < started_at),
                    (SELECT COALESCE(SUM(input_tokens),0) FROM telemetry_turns),
                    (SELECT COALESCE(SUM(input_tokens),0) FROM telemetry_rollup_hourly)",
        [],
        |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
            ))
        },
    )?;
    require(turns > 0, "log must yield telemetry turns")?;
    require(intervals > 0, "log must yield activity intervals")?;
    require(inverted == 0, "activity intervals must be forward spans")?;
    require(
        fact_input == rollup_input,
        "rollups disagree with token facts",
    )?;
    Ok((turns, edits, intervals))
}
