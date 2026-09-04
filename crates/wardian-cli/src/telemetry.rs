//! `wardian telemetry` — read the habitat telemetry store from a shell.
//!
//! Ingest belongs to the running app, which owns the cursors; a CLI that
//! advanced them too would let two writers race for the same source. The CLI
//! remains read-only; destructive telemetry maintenance is owned by the core
//! application service.

use crate::args::{TelemetryArgs, TelemetryCommand};
use crate::errors::CliError;
use crate::open_db;
use wardian_core::telemetry::horizon::{resolve_horizon, Horizon, HorizonWindow};
use wardian_core::telemetry::models::{ActiveTime, BreakdownRow, TokenCounts};
use wardian_core::telemetry::query::{breakdown, latest_limits, summary, Dimension};

pub fn handle_telemetry(args: TelemetryArgs) -> Result<String, CliError> {
    match args.command {
        TelemetryCommand::Summary { horizon, dimension } => {
            let horizon = parse_horizon(&horizon)?;
            let dimension = parse_dimension(&dimension)?;
            let window = resolve_horizon(horizon, chrono::Utc::now());
            let conn = open_db()?;

            let totals = summary(&conn, &window.from, &window.to).map_err(db_error)?;
            let mut rows = breakdown(
                &conn,
                dimension,
                &window.from,
                &window.to,
                BREAKDOWN_LIMIT + 1,
            )
            .map_err(db_error)?;
            let truncated = rows.len() > BREAKDOWN_LIMIT;
            rows.truncate(BREAKDOWN_LIMIT);
            let limits = latest_limits(&conn).map_err(db_error)?;

            render_json(serde_json::json!({
                "schema": 1,
                "horizon": horizon.as_str(),
                "window": window_json(&window),
                "summary": {
                    "active": active_json(&totals.active),
                    "turns": totals.turns,
                    "tokens": tokens_json(&totals.tokens),
                    "processed_tokens": totals.tokens.processed_total(),
                    "files_touched": totals.files_touched,
                    "lines_added": totals.lines_added,
                    "lines_removed": totals.lines_removed,
                    "agent_count": totals.agent_count,
                },
                "dimension": dimension.as_str(),
                "rows": rows.iter().map(row_json).collect::<Vec<_>>(),
                "row_limit": BREAKDOWN_LIMIT,
                "truncated": truncated,
                "limits": limits,
            }))
        }
    }
}

/// Matches the Dashboard, so the two do not disagree about where the tail ends.
const BREAKDOWN_LIMIT: usize = 24;

fn parse_horizon(value: &str) -> Result<Horizon, CliError> {
    Horizon::parse(value).ok_or_else(|| {
        CliError::generic(format!(
            "Unknown horizon '{value}'. Expected one of: today, day (24h), week (7d), month (30d), all."
        ))
    })
}

fn parse_dimension(value: &str) -> Result<Dimension, CliError> {
    Dimension::parse(value).ok_or_else(|| {
        CliError::generic(format!(
            "Unknown dimension '{value}'. Expected one of: provider, agent, model."
        ))
    })
}

fn window_json(window: &HorizonWindow) -> serde_json::Value {
    serde_json::json!({
        "from": window.from,
        "to": window.to,
        // Reported rather than hidden: the window really is up to an hour wider
        // than the horizon's name suggests, because buckets are hourly.
        "from_floored": window.from_floored,
    })
}

/// Active time, never blended into one figure.
///
/// `measured_ms` is a duration a provider reported; `clustered_ms` is inferred
/// from gaps between events and systematically runs high. A consumer that wants
/// one number has to decide what to call it, and `mixed` tells it whether that
/// decision is even defensible here.
fn active_json(active: &ActiveTime) -> serde_json::Value {
    serde_json::json!({
        "measured_ms": active.measured_ms,
        "clustered_ms": active.clustered_ms,
        "mixed": active.is_mixed(),
    })
}

/// Token components, each independently nullable.
///
/// Emitted as `null` rather than `0` when unreported. A provider with no token
/// accounting has not used zero tokens, and a consumer sorting on these must be
/// able to tell the difference.
fn tokens_json(tokens: &TokenCounts) -> serde_json::Value {
    serde_json::json!({
        "input_tokens": tokens.input_tokens,
        // Kept separate from `input_tokens` on purpose: cache reads ran several
        // times fresh input on real sessions, so a consumer that added them
        // together would overstate usage by close to an order of magnitude.
        "cached_input_tokens": tokens.cached_input_tokens,
        "cache_write_tokens": tokens.cache_write_tokens,
        "output_tokens": tokens.output_tokens,
        "reasoning_tokens": tokens.reasoning_tokens,
    })
}

fn row_json(row: &BreakdownRow) -> serde_json::Value {
    serde_json::json!({
        "key": row.key,
        "active": active_json(&row.active),
        "turns": row.turns,
        "tokens": tokens_json(&row.tokens),
        "processed_tokens": row.tokens.processed_total(),
        "tokens_reported": row.tokens_reported,
        "files_touched": row.files_touched,
        "lines_added": row.lines_added,
        "lines_removed": row.lines_removed,
        "agent_count": row.agent_count,
    })
}

fn db_error(error: rusqlite::Error) -> CliError {
    CliError::db_unavailable(error.to_string())
}

fn render_json(body: serde_json::Value) -> Result<String, CliError> {
    serde_json::to_string(&body)
        .map(|json| format!("{json}\n"))
        .map_err(|error| CliError::generic(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn horizon_names_are_accepted_in_both_spellings() {
        assert_eq!(parse_horizon("7d").unwrap(), Horizon::Week);
        assert_eq!(parse_horizon("week").unwrap(), Horizon::Week);
        assert!(parse_horizon("fortnight").is_err());
    }

    #[test]
    fn dimension_names_are_validated_before_reaching_sql() {
        // `Dimension::column()` is interpolated into the query, so this is the
        // gate that keeps a caller-supplied string out of it.
        assert_eq!(parse_dimension("agent").unwrap(), Dimension::Agent);
        assert!(parse_dimension("provider; DROP TABLE telemetry_turns").is_err());
    }

    #[test]
    fn active_time_is_emitted_split_and_flagged() {
        let json = active_json(&ActiveTime {
            measured_ms: 600_000,
            clustered_ms: 1_800_000,
        });
        // No blended total: a consumer that wants one has to opt into saying
        // what it means.
        assert_eq!(json["measured_ms"], 600_000);
        assert_eq!(json["clustered_ms"], 1_800_000);
        assert_eq!(json["mixed"], true);
        assert!(json.get("active_ms").is_none());
    }

    #[test]
    fn unreported_tokens_serialize_as_null_not_zero() {
        // The antigravity case. A zero here would rank it the cheapest provider
        // rather than the unmeasured one.
        let json = tokens_json(&TokenCounts::default());
        assert!(json["input_tokens"].is_null());
        assert!(json["output_tokens"].is_null());
    }

    #[test]
    fn cache_reads_stay_out_of_the_processed_total() {
        let row = BreakdownRow {
            key: "codex".into(),
            tokens: TokenCounts {
                input_tokens: Some(100_544),
                cached_input_tokens: Some(730_880),
                output_tokens: Some(5_254),
                ..TokenCounts::default()
            },
            tokens_reported: true,
            ..BreakdownRow::default()
        };
        let json = row_json(&row);
        assert_eq!(json["processed_tokens"], 105_798);
        // Still addressable, just not in the headline figure.
        assert_eq!(json["tokens"]["cached_input_tokens"], 730_880);
    }

    #[test]
    fn cache_writes_are_inside_the_processed_total() {
        // Claude-shaped traffic, where nearly all fresh prompt content is
        // written into the cache rather than sent as plain input. Counting only
        // input and output would report 471,400 for 2,357,271 tokens of work.
        let row = BreakdownRow {
            key: "claude".into(),
            tokens: TokenCounts {
                input_tokens: Some(8_446),
                cached_input_tokens: Some(80_194_623),
                cache_write_tokens: Some(1_885_871),
                output_tokens: Some(462_954),
                reasoning_tokens: None,
            },
            tokens_reported: true,
            ..BreakdownRow::default()
        };
        let json = row_json(&row);
        assert_eq!(json["processed_tokens"], 2_357_271);
        assert_eq!(json["tokens"]["cache_write_tokens"], 1_885_871);
    }
}
