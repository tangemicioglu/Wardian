//! Read commands over the habitat telemetry store.
//!
//! Every command here answers from `telemetry_rollup_hourly` by way of
//! [`wardian_core::telemetry::query`], so a horizon costs one row per hour per
//! agent per model rather than a re-read of the provider logs.
//!
//! Commands take a named horizon rather than raw timestamps, and resolve it
//! through [`wardian_core::telemetry::horizon`]. Buckets are hourly, so a
//! caller-supplied `from` that lands mid-hour would silently drop that whole
//! hour — the surface would show a figure that was simply wrong rather than one
//! it could recognize as truncated. Keeping the rule in the core means the CLI
//! answers the same question the same way.

use crate::state::AppState;
use chrono::Utc;
use serde::Serialize;
use tauri::State;
// Horizon resolution lives in the core so the CLI resolves "the last 24 hours"
// the same way this does. Two independent implementations of the flooring rule
// would let two surfaces quote different figures for the same question.
use wardian_core::telemetry::horizon::{resolve_horizon, Horizon, HorizonWindow};
use wardian_core::telemetry::models::{
    ActiveTime, BreakdownRow, IntervalFact, LimitObservation, TelemetrySummary, TokenCounts,
};
use wardian_core::telemetry::matrix::{matrix_at, Measure};
use wardian_core::telemetry::query::{
    activity_intervals, breakdown, latest_limits, series, summary, Dimension, SeriesPoint,
};

/// A breakdown row with the label a surface should actually print.
///
/// `key` stays the store's own value so the UI can correlate rows across calls;
/// `label` is what a person reads. For agents these differ — the store keys on a
/// session UUID, which is meaningless on screen.
#[derive(Debug, Clone, Serialize)]
pub struct TelemetryBreakdownRowDto {
    pub key: String,
    pub label: String,
    pub active: ActiveTime,
    pub turns: i64,
    pub tokens: TokenCounts,
    pub billable_tokens: Option<i64>,
    pub files_touched: i64,
    pub lines_added: i64,
    pub lines_removed: i64,
    pub agent_count: i64,
    pub tokens_reported: bool,
}

/// One agent's figures for a horizon, several measures at once.
///
/// The Dashboard's unit. Deliberately wide rather than one measure at a time:
/// the question is almost always "who has been doing what lately", and
/// answering it from separate per-measure lists makes the reader join them by
/// hand.
#[derive(Debug, Clone, Serialize)]
pub struct TelemetryAgentRowDto {
    pub key: String,
    pub label: String,
    pub sublabel: Option<String>,
    /// Measured and inferred durations summed. The split is preserved in the
    /// store but is not worth a column.
    pub active_ms: i64,
    pub turns: i64,
    /// Fresh input plus output — new content processed, cache reads excluded.
    pub total_tokens: Option<i64>,
    pub cached_tokens: Option<i64>,
    pub files_touched: i64,
    pub lines_added: i64,
    pub lines_removed: i64,
    /// False when no contributing provider reported token accounting, so the
    /// row reads as unmeasured rather than as zero.
    pub tokens_reported: bool,
    /// True when this agent recorded nothing in the window. It is still listed,
    /// because a roster that hides its quiet members answers a different
    /// question from the one being asked.
    pub idle: bool,
    /// The selected measure bucketed across the window, one value per entry in
    /// [`TelemetryDashboardDto::buckets`].
    ///
    /// A row's figures are a total, and a total cannot tell a steady week from
    /// one frantic afternoon. This is what makes the Dashboard answer "across
    /// time" without leaving the surface.
    pub spark: Vec<i64>,
}

/// One provider's contribution to the habitat, plus its account headroom.
///
/// Capacity lives here rather than in a component of its own so that the
/// Dashboard has the same shape whatever a habitat runs. A provider that
/// publishes no limit reports `limits: []`, which reads as "not reported" beside
/// providers that do — the same treatment every other unreported measure gets,
/// instead of the element vanishing.
#[derive(Debug, Clone, Serialize)]
pub struct TelemetryProviderRowDto {
    pub provider: String,
    /// How many agents on this provider recorded anything in the window.
    pub agent_count: i64,
    pub active_ms: i64,
    pub turns: i64,
    pub total_tokens: Option<i64>,
    pub files_touched: i64,
    pub lines_added: i64,
    pub lines_removed: i64,
    /// False when this provider publishes no token accounting at all.
    pub tokens_reported: bool,
    /// Account gauges for this provider. Never per-agent: two agents on one
    /// account observe the same figure, so these are never summed.
    pub limits: Vec<LimitObservation>,
}

/// The Dashboard payload: every agent, plus account-level state.
#[derive(Debug, Clone, Serialize)]
pub struct TelemetryDashboardDto {
    pub window: HorizonWindow,
    pub rows: Vec<TelemetryAgentRowDto>,
    /// One row per provider the habitat actually used in this window.
    ///
    /// A structural element, not a conditional one. Account headroom used to be
    /// its own component, which meant the Dashboard grew and lost a whole block
    /// depending on which provider happened to be in use — codex publishes a
    /// limit and nothing else does, so the layout was effectively codex-shaped.
    /// Capacity is now a *field* on a provider row, absent the way any other
    /// unreported measure is absent.
    pub providers: Vec<TelemetryProviderRowDto>,
    /// Bucket start instants for every row's `spark`, ascending.
    pub buckets: Vec<String>,
    /// Which measure the sparklines carry.
    pub spark_measure: String,
    /// Bucket width, so a surface can say what one spark column covers.
    pub grain: String,
    /// Largest single bucket across every row, so sparklines share one scale
    /// and a busy agent looks busier than a quiet one instead of every row
    /// being normalized to its own maximum.
    pub spark_max: i64,
}

/// One row of the matrix, carrying the name a person reads.
///
/// `label` is separate from `key` because the store keys agents on a session
/// UUID. Rendering the key is what put raw UUIDs down the side of the first
/// Dashboard; every row that reaches a surface goes through [`label_for`].
#[derive(Debug, Clone, Serialize)]
pub struct TelemetryMatrixRowDto {
    pub key: String,
    pub label: String,
    /// Secondary line — an agent's class, or the provider behind a model.
    pub sublabel: Option<String>,
    pub cells: Vec<i64>,
    pub total: i64,
}

/// A dense rows × buckets grid for one measure.
#[derive(Debug, Clone, Serialize)]
pub struct TelemetryMatrixDto {
    pub dimension: String,
    pub measure: String,
    pub grain: String,
    pub window: HorizonWindow,
    pub buckets: Vec<String>,
    pub rows: Vec<TelemetryMatrixRowDto>,
    pub max_cell: i64,
    /// True when the measure is a distinct count, so a row's cells do not sum
    /// to its total and a surface must not present them as if they did.
    pub cells_are_not_additive: bool,
}

/// Everything the Dashboard needs for one horizon, in one round trip.
#[derive(Debug, Clone, Serialize)]
pub struct TelemetryOverviewDto {
    pub window: HorizonWindow,
    pub summary: TelemetrySummary,
    /// Fresh input plus output. Computed once here so every surface shows the
    /// same figure rather than each re-deriving what "tokens used" means.
    pub billable_tokens: Option<i64>,
    /// True when measured and clustered durations both contributed, so any
    /// single active-time figure shown is a mixture of a measurement and an
    /// estimate and has to be labelled as one.
    pub active_is_mixed: bool,
    pub by_provider: Vec<TelemetryBreakdownRowDto>,
    pub by_agent: Vec<TelemetryBreakdownRowDto>,
    pub by_model: Vec<TelemetryBreakdownRowDto>,
    pub limits: Vec<LimitObservation>,
}

/// How many rows a breakdown returns before the tail is dropped.
const BREAKDOWN_LIMIT: usize = 24;

/// How many columns a Dashboard sparkline may have.
///
/// The Analytics matrix aims for roughly a hundred because it is an axis values
/// get read off. A sparkline is a shape glanced at inside a table row, so it
/// gets a coarser grain — chosen by bucket count rather than fixed width, so
/// that a four-hour horizon still resolves to five-minute columns instead of
/// four hourly ones.
const SPARK_BUCKETS: usize = 48;

/// Every read here is `#[tauri::command(async)]` on a synchronous body.
///
/// That combination is deliberate. A plain `#[tauri::command]` on a sync
/// function runs on the **main thread**, and these reads take the global
/// database mutex — which an ingest pass can hold for as long as its timeout
/// allows while it works through a large backlog. On the main thread that is a
/// frozen window, not a slow query. `(async)` moves the same synchronous body
/// onto the runtime's blocking pool, where waiting for the mutex costs nothing
/// but the caller's own latency.
#[tauri::command(async)]
pub fn telemetry_overview(horizon: String) -> Result<TelemetryOverviewDto, String> {
    let horizon = Horizon::parse(&horizon)
        .ok_or_else(|| format!("unknown telemetry horizon: {horizon}"))?;
    let window = resolve_horizon(horizon, Utc::now());
    let labels = agent_labels();

    wardian_core::db::get_db_conn(|conn| {
        let summary = summary(conn, &window.from, &window.to)?;
        let rows = |dimension| -> Result<Vec<BreakdownRow>, Box<dyn std::error::Error>> {
            Ok(breakdown(
                conn,
                dimension,
                &window.from,
                &window.to,
                BREAKDOWN_LIMIT,
            )?)
        };
        Ok(TelemetryOverviewDto {
            billable_tokens: summary.tokens.billable_total(),
            active_is_mixed: summary.active.is_mixed(),
            by_provider: to_dtos(rows(Dimension::Provider)?, Dimension::Provider, &labels),
            by_agent: to_dtos(rows(Dimension::Agent)?, Dimension::Agent, &labels),
            by_model: to_dtos(rows(Dimension::Model)?, Dimension::Model, &labels),
            limits: latest_limits(conn)?,
            summary,
            window,
        })
    })
    .map_err(|error| format!("could not read telemetry overview: {error}"))
}

/// Every agent's figures for a horizon, for the Dashboard.
///
/// See [`telemetry_overview`] for why this is `(async)`.
#[tauri::command(async)]
pub fn telemetry_dashboard(
    horizon: String,
    measure: Option<String>,
) -> Result<TelemetryDashboardDto, String> {
    let horizon =
        Horizon::parse(&horizon).ok_or_else(|| format!("unknown telemetry horizon: {horizon}"))?;
    let window = resolve_horizon(horizon, Utc::now());
    let labels = agent_labels();
    let spark_measure = match measure.as_deref() {
        None => Measure::ActiveMs,
        Some(name) => {
            Measure::parse(name).ok_or_else(|| format!("unknown telemetry measure: {name}"))?
        }
    };

    wardian_core::db::get_db_conn(|conn| {
        // No practical limit: the roster is the answer, so truncating it would
        // silently drop agents from a view whose job is to list them.
        let measured = breakdown(conn, Dimension::Agent, &window.from, &window.to, usize::MAX)?;

        // One grid for every agent's sparkline, rather than a query per row.
        let grid = matrix_at(
            conn,
            &window,
            Dimension::Agent,
            spark_measure,
            usize::MAX,
            Some(SPARK_BUCKETS),
        )?;
        let bucket_count = grid.buckets.len();
        let sparks: std::collections::HashMap<String, Vec<i64>> = grid
            .rows
            .into_iter()
            .map(|row| (row.key, row.cells))
            .collect();

        let mut rows: Vec<TelemetryAgentRowDto> = measured
            .into_iter()
            .map(|row| TelemetryAgentRowDto {
                spark: sparks
                    .get(&row.key)
                    .cloned()
                    .unwrap_or_else(|| vec![0; bucket_count]),
                label: label_for(&row.key, Dimension::Agent, &labels),
                sublabel: sublabel_for(&row.key, Dimension::Agent, &labels),
                active_ms: row.active.measured_ms + row.active.clustered_ms,
                turns: row.turns,
                total_tokens: row.tokens.billable_total(),
                cached_tokens: row.tokens.cached_input_tokens,
                files_touched: row.files_touched,
                lines_added: row.lines_added,
                lines_removed: row.lines_removed,
                tokens_reported: row.tokens_reported,
                idle: false,
                key: row.key,
            })
            .collect();

        // Seed the agents that recorded nothing. An agent quiet this week is a
        // fact about the week, and dropping its row makes the roster look
        // smaller than the habitat actually is — which is exactly how the first
        // attempt showed four agents out of fifty-four.
        let seen: std::collections::HashSet<&str> =
            rows.iter().map(|row| row.key.as_str()).collect();
        let idle: Vec<TelemetryAgentRowDto> = labels
            .iter()
            .filter(|(key, _)| !seen.contains(key.as_str()))
            .map(|(key, label)| TelemetryAgentRowDto {
                key: key.clone(),
                label: label.name.clone(),
                sublabel: label.class.clone(),
                active_ms: 0,
                turns: 0,
                // Nothing recorded is not a report of zero tokens.
                total_tokens: None,
                cached_tokens: None,
                files_touched: 0,
                lines_added: 0,
                lines_removed: 0,
                tokens_reported: false,
                idle: true,
                spark: vec![0; bucket_count],
            })
            .collect();
        rows.extend(idle);

        // One scale across the whole table. Normalizing each row to its own
        // maximum would draw a agent that ran for ten minutes and one that ran
        // all week as the same shape.
        let spark_max = rows
            .iter()
            .flat_map(|row| row.spark.iter().copied())
            .max()
            .unwrap_or(0);

        // Account gauges are keyed by provider, so they attach to the provider
        // row rather than floating beside the table with nothing to belong to.
        let all_limits = latest_limits(conn)?;
        let providers: Vec<TelemetryProviderRowDto> =
            breakdown(conn, Dimension::Provider, &window.from, &window.to, usize::MAX)?
                .into_iter()
                .map(|row| TelemetryProviderRowDto {
                    active_ms: row.active.measured_ms + row.active.clustered_ms,
                    turns: row.turns,
                    total_tokens: row.tokens.billable_total(),
                    files_touched: row.files_touched,
                    lines_added: row.lines_added,
                    lines_removed: row.lines_removed,
                    tokens_reported: row.tokens_reported,
                    agent_count: row.agent_count,
                    limits: all_limits
                        .iter()
                        .filter(|limit| limit.provider == row.key)
                        .cloned()
                        .collect(),
                    provider: row.key,
                })
                .collect();

        Ok(TelemetryDashboardDto {
            providers,
            rows,
            window,
            buckets: grid.buckets,
            spark_measure: spark_measure.as_str().to_string(),
            grain: grid.grain.as_str().to_string(),
            spark_max,
        })
    })
    .map_err(|error| format!("could not read telemetry dashboard: {error}"))
}

/// One agent's live rates over the Dashboard's trailing window.
///
/// Rates rather than totals, deliberately. The Dashboard is a process viewer,
/// and a process viewer shows consumption *now*, not consumption since boot: a
/// cumulative figure ranks history, where a rate makes a runaway obvious.
/// Totals are still carried for the columns denominated that way, and their
/// header names the window they cover.
#[derive(Debug, Clone, Serialize)]
pub struct TelemetryFleetRowDto {
    pub key: String,
    pub label: String,
    pub sublabel: Option<String>,
    /// Billable tokens per hour. `None` when the provider reports no tokens.
    pub tokens_per_hour: Option<f64>,
    pub turns_per_hour: f64,
    pub active_ms: i64,
    pub turns: i64,
    pub total_tokens: Option<i64>,
    pub files_touched: i64,
    pub lines_added: i64,
    pub lines_removed: i64,
    pub tokens_reported: bool,
    /// True when nothing was recorded in the window. Still listed: on a resource
    /// monitor an idle agent is available capacity, which is the answer to
    /// "where can I spend what is left".
    pub idle: bool,
    /// The trend measure per bucket, aligned to [`TelemetryFleetDto::buckets`].
    pub spark: Vec<i64>,
}

/// The largest value in each column across the whole table.
///
/// Every scaled visual normalizes against these rather than against its own row.
/// Most columns have no absolute ceiling — only codex publishes a limit — so on
/// a fleet monitor **the fleet is the denominator**. Spotting a runaway does not
/// need a ceiling, it needs an outlier.
#[derive(Debug, Clone, Default, Serialize)]
pub struct TelemetryFleetMaximaDto {
    pub tokens_per_hour: f64,
    pub turns_per_hour: f64,
    pub turns: i64,
    pub active_ms: i64,
    pub total_tokens: i64,
    pub files_touched: i64,
    pub lines: i64,
    pub spark: i64,
}

/// The Dashboard payload: every agent's rates over one trailing window.
#[derive(Debug, Clone, Serialize)]
pub struct TelemetryFleetDto {
    pub window: HorizonWindow,
    pub window_minutes: i64,
    pub rows: Vec<TelemetryFleetRowDto>,
    pub maxima: TelemetryFleetMaximaDto,
    pub buckets: Vec<String>,
    pub trend_measure: String,
    pub grain: String,
}

/// Narrowest window the Dashboard will read, in minutes.
///
/// Below this the rate is dominated by whether a single turn happened to land
/// inside the window, which reports noise as a signal.
const MIN_FLEET_WINDOW_MINUTES: i64 = 5;

/// Widest window, in minutes.
///
/// Generous rather than principled. Capping this at a day was defensible on the
/// theory that anything longer belongs to Analytics, and simply hid an agent
/// that worked hard earlier in the week from a surface whose job is to say what
/// the habitat has been doing.
const MAX_FLEET_WINDOW_MINUTES: i64 = 90 * 24 * 60;

/// How many columns the Dashboard sparkline may have.
const FLEET_SPARK_BUCKETS: usize = 48;

/// Every agent's rates over a trailing window.
///
/// Reads facts rather than rollups. The Dashboard's window is typically an hour,
/// and an hourly rollup cannot answer a question that fine — it would report a
/// whole hour's work as one undivided block, or miss it entirely.
///
/// See [`telemetry_overview`] for why this is `(async)`.
#[tauri::command(async)]
pub fn telemetry_fleet(
    window_minutes: Option<i64>,
    measure: Option<String>,
) -> Result<TelemetryFleetDto, String> {
    let window_minutes = window_minutes
        .unwrap_or(60)
        .clamp(MIN_FLEET_WINDOW_MINUTES, MAX_FLEET_WINDOW_MINUTES);
    let trend_measure = match measure.as_deref() {
        None => Measure::TotalTokens,
        Some(name) => {
            Measure::parse(name).ok_or_else(|| format!("unknown telemetry measure: {name}"))?
        }
    };

    let to = Utc::now();
    let from = to - chrono::Duration::minutes(window_minutes);
    let window = HorizonWindow {
        from: from.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        to: to.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        // A trailing window is exact; nothing is floored to a bucket boundary,
        // which is precisely why this cannot be answered from the rollups.
        from_floored: false,
    };
    let labels = agent_labels();
    let hours = (window_minutes as f64) / 60.0;

    wardian_core::db::get_db_conn(|conn| {
        // One grid per measure. Each carries both the per-bucket cells and a
        // window total that is correct for its measure — a distinct count does
        // not sum across buckets, so the totals cannot be derived from cells.
        let grid = |measure| {
            matrix_at(
                conn,
                &window,
                Dimension::Agent,
                measure,
                usize::MAX,
                Some(FLEET_SPARK_BUCKETS),
            )
        };

        let trend = grid(trend_measure)?;
        let totals_for = |measure| -> Result<std::collections::HashMap<String, i64>, _> {
            grid(measure).map(|matrix| {
                matrix
                    .rows
                    .into_iter()
                    .map(|row| (row.key, row.total))
                    .collect()
            })
        };

        let active = totals_for(Measure::ActiveMs)?;
        let turns = totals_for(Measure::Turns)?;
        let tokens = totals_for(Measure::TotalTokens)?;
        let files = totals_for(Measure::Files)?;
        let added = totals_for(Measure::LinesAdded)?;
        let removed = totals_for(Measure::LinesRemoved)?;

        // Which agents can report tokens at all, so an unmeasured provider reads
        // as absent rather than as the quietest agent in the habitat.
        //
        // Read from the facts over this exact window, never from `breakdown`.
        // That reads `telemetry_rollup_hourly WHERE bucket_start >= from`, and a
        // trailing window shorter than an hour begins *inside* a bucket whose
        // start precedes it — so a 15-minute view matched no rollup row at all
        // and turned every measured token total into "unreported", hiding the
        // burn rates this surface exists to show.
        let reported = token_reporting_agents(conn, &window.from, &window.to)?;

        let bucket_count = trend.buckets.len();
        let sparks: std::collections::HashMap<String, Vec<i64>> = trend
            .rows
            .into_iter()
            .map(|row| (row.key, row.cells))
            .collect();

        let mut rows: Vec<TelemetryFleetRowDto> = labels
            .iter()
            .map(|(key, label)| {
                let turns_total = turns.get(key).copied().unwrap_or(0);
                let tokens_total = tokens.get(key).copied().unwrap_or(0);
                let active_ms = active.get(key).copied().unwrap_or(0);
                let files_touched = files.get(key).copied().unwrap_or(0);
                let lines_added = added.get(key).copied().unwrap_or(0);
                let lines_removed = removed.get(key).copied().unwrap_or(0);
                let tokens_reported = reported.contains(key);

                TelemetryFleetRowDto {
                    key: key.clone(),
                    label: label.name.clone(),
                    sublabel: label.class.clone(),
                    tokens_per_hour: tokens_reported
                        .then(|| (tokens_total as f64) / hours.max(f64::EPSILON)),
                    turns_per_hour: (turns_total as f64) / hours.max(f64::EPSILON),
                    active_ms,
                    turns: turns_total,
                    total_tokens: tokens_reported.then_some(tokens_total),
                    files_touched,
                    lines_added,
                    lines_removed,
                    tokens_reported,
                    idle: turns_total == 0
                        && active_ms == 0
                        && files_touched == 0
                        && tokens_total == 0,
                    spark: sparks
                        .get(key)
                        .cloned()
                        .unwrap_or_else(|| vec![0; bucket_count]),
                }
            })
            .collect();

        rows.sort_by(|left, right| left.label.cmp(&right.label));

        let mut maxima = TelemetryFleetMaximaDto::default();
        for row in &rows {
            maxima.tokens_per_hour = maxima.tokens_per_hour.max(row.tokens_per_hour.unwrap_or(0.0));
            maxima.turns_per_hour = maxima.turns_per_hour.max(row.turns_per_hour);
            maxima.turns = maxima.turns.max(row.turns);
            maxima.active_ms = maxima.active_ms.max(row.active_ms);
            maxima.total_tokens = maxima.total_tokens.max(row.total_tokens.unwrap_or(0));
            maxima.files_touched = maxima.files_touched.max(row.files_touched);
            maxima.lines = maxima.lines.max(row.lines_added + row.lines_removed);
            maxima.spark = maxima
                .spark
                .max(row.spark.iter().copied().max().unwrap_or(0));
        }

        Ok(TelemetryFleetDto {
            window,
            window_minutes,
            rows,
            maxima,
            buckets: trend.buckets,
            trend_measure: trend_measure.as_str().to_string(),
            grain: trend.grain.as_str().to_string(),
        })
    })
    .map_err(|error| format!("could not read telemetry fleet: {error}"))
}

/// Agents whose provider reported any token accounting inside an exact window.
///
/// Fact-backed rather than rollup-backed, because the Dashboard's window is a
/// trailing interval that routinely starts mid-hour, and an hourly bucket whose
/// start precedes the window is invisible to a `bucket_start >= from` filter.
///
/// Presence, not magnitude: an agent that genuinely burned zero tokens in the
/// window still belongs here, because "reported zero" and "reports nothing" are
/// different claims and only the second may render as unknown.
fn token_reporting_agents(
    conn: &rusqlite::Connection,
    from: &str,
    to: &str,
) -> rusqlite::Result<std::collections::HashSet<String>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT session_id FROM telemetry_turns
         WHERE ended_at >= ?1 AND ended_at < ?2
           AND (input_tokens IS NOT NULL
                OR output_tokens IS NOT NULL
                OR cached_input_tokens IS NOT NULL)",
    )?;
    let rows = stmt.query_map(rusqlite::params![from, to], |row| row.get::<_, String>(0))?;
    rows.collect()
}

/// Read the Dashboard's saved column and window preferences.
///
/// Deliberately untyped at this boundary, and stored the same way the watchlist
/// stores its own: the frontend merges whatever is here *over* its defaults, so
/// a column added in a later release appears without a migration and a stale
/// file can never hide one. Returning `Null` on a first run is a normal answer.
#[tauri::command]
pub async fn load_dashboard_prefs() -> Result<serde_json::Value, String> {
    if let Some(home) = crate::utils::fs::get_wardian_home() {
        let path = home.join("settings/dashboard-prefs.json");
        if let Ok(data) = std::fs::read_to_string(&path) {
            return Ok(serde_json::from_str(&data).unwrap_or(serde_json::Value::Null));
        }
    }
    Ok(serde_json::Value::Null)
}

/// Persist the Dashboard preferences.
///
/// Written on every change rather than behind a save button. This is the seed a
/// *new* Dashboard instance starts from; instances already open keep their own
/// state and are never rewritten by this.
#[tauri::command]
pub async fn save_dashboard_prefs(prefs: serde_json::Value) -> Result<(), String> {
    let home = crate::utils::fs::get_wardian_home()
        .ok_or_else(|| "Could not find home directory".to_string())?;
    let _ = std::fs::create_dir_all(home.join("settings"));
    let json = serde_json::to_string_pretty(&prefs).map_err(|error| error.to_string())?;
    std::fs::write(home.join("settings/dashboard-prefs.json"), json)
        .map_err(|error| error.to_string())?;
    Ok(())
}

/// The matrix Analytics is built on: rows × time, for one measure.
///
/// See [`telemetry_overview`] for why this is `(async)`.
#[tauri::command(async)]
pub fn telemetry_matrix(
    horizon: String,
    dimension: String,
    measure: String,
    limit: Option<usize>,
) -> Result<TelemetryMatrixDto, String> {
    let horizon =
        Horizon::parse(&horizon).ok_or_else(|| format!("unknown telemetry horizon: {horizon}"))?;
    let dimension = Dimension::parse(&dimension)
        .ok_or_else(|| format!("unknown telemetry dimension: {dimension}"))?;
    let measure =
        Measure::parse(&measure).ok_or_else(|| format!("unknown telemetry measure: {measure}"))?;
    let window = resolve_horizon(horizon, Utc::now());
    let labels = agent_labels();
    let row_limit = limit.unwrap_or(MATRIX_ROW_LIMIT).clamp(1, MATRIX_ROW_CAP);

    wardian_core::db::get_db_conn(|conn| {
        let grid = wardian_core::telemetry::matrix::matrix(
            conn, &window, dimension, measure, row_limit,
        )?;
        Ok(TelemetryMatrixDto {
            dimension: dimension.as_str().to_string(),
            measure: measure.as_str().to_string(),
            grain: grid.grain.as_str().to_string(),
            rows: grid
                .rows
                .into_iter()
                .map(|row| TelemetryMatrixRowDto {
                    label: label_for(&row.key, dimension, &labels),
                    sublabel: sublabel_for(&row.key, dimension, &labels),
                    key: row.key,
                    cells: row.cells,
                    total: row.total,
                })
                .collect(),
            buckets: grid.buckets,
            max_cell: grid.max_cell,
            cells_are_not_additive: grid.cells_are_not_additive,
            window,
        })
    })
    .map_err(|error| format!("could not read telemetry matrix: {error}"))
}

/// Rows returned when a caller does not say.
const MATRIX_ROW_LIMIT: usize = 40;

/// Hard ceiling, so a caller cannot ask for an unbounded grid.
const MATRIX_ROW_CAP: usize = 200;

/// See [`telemetry_overview`] for why this is `(async)`.
#[tauri::command(async)]
pub fn telemetry_series(horizon: String, dimension: String) -> Result<Vec<SeriesPoint>, String> {
    let horizon = Horizon::parse(&horizon)
        .ok_or_else(|| format!("unknown telemetry horizon: {horizon}"))?;
    let dimension = Dimension::parse(&dimension)
        .ok_or_else(|| format!("unknown telemetry dimension: {dimension}"))?;
    let window = resolve_horizon(horizon, Utc::now());

    wardian_core::db::get_db_conn(|conn| Ok(series(conn, dimension, &window.from, &window.to)?))
        .map_err(|error| format!("could not read telemetry series: {error}"))
}

/// See [`telemetry_overview`] for why this is `(async)`.
#[tauri::command(async)]
pub fn telemetry_activity(horizon: String) -> Result<Vec<IntervalFact>, String> {
    let horizon = Horizon::parse(&horizon)
        .ok_or_else(|| format!("unknown telemetry horizon: {horizon}"))?;
    let window = resolve_horizon(horizon, Utc::now());

    wardian_core::db::get_db_conn(|conn| Ok(activity_intervals(conn, &window.from, &window.to)?))
        .map_err(|error| format!("could not read telemetry activity: {error}"))
}

/// Advance every source now, rather than waiting for the scheduled pass.
///
/// Exists so a surface can offer a refresh that means something. Ingest is
/// idempotent, so an extra pass costs a cursor comparison per source.
#[tauri::command]
pub async fn telemetry_refresh(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    let report = crate::state::telemetry_ingest::run_ingest_cycle(&state).await;
    Ok(serde_json::json!({
        "sources": report.sources,
        "advanced": report.advanced,
        "turns": report.turns,
        "edits": report.edits,
        "intervals": report.intervals,
        "buckets_recomputed": report.buckets_recomputed,
        "unavailable": report.unavailable,
        "failures": report.failures,
    }))
}

fn to_dtos(
    rows: Vec<BreakdownRow>,
    dimension: Dimension,
    labels: &AgentLabels,
) -> Vec<TelemetryBreakdownRowDto> {
    rows.into_iter()
        .map(|row| TelemetryBreakdownRowDto {
            label: label_for(&row.key, dimension, labels),
            billable_tokens: row.tokens.billable_total(),
            key: row.key,
            active: row.active,
            turns: row.turns,
            tokens: row.tokens,
            files_touched: row.files_touched,
            lines_added: row.lines_added,
            lines_removed: row.lines_removed,
            agent_count: row.agent_count,
            tokens_reported: row.tokens_reported,
        })
        .collect()
}

/// What to print for a key.
///
/// A model row can carry an empty key, because `model` is nullable in the rollup
/// and SQL groups nulls together. That is a real state — a turn whose model was
/// never stated — and it has to read as unknown rather than as a blank row.
fn label_for(key: &str, dimension: Dimension, labels: &AgentLabels) -> String {
    if key.is_empty() {
        return match dimension {
            Dimension::Model => "Unknown model".to_string(),
            Dimension::Provider => "Unknown provider".to_string(),
            Dimension::Agent => "Unknown agent".to_string(),
        };
    }
    match dimension {
        // A session UUID is not a name. Falling back to the key keeps a deleted
        // agent's history addressable instead of dropping the row.
        Dimension::Agent => labels
            .get(key)
            .map(|label| label.name.clone())
            .unwrap_or_else(|| key.to_string()),
        _ => key.to_string(),
    }
}

/// What a surface should print for one agent.
#[derive(Debug, Clone)]
pub struct AgentLabel {
    pub name: String,
    pub class: Option<String>,
}

type AgentLabels = std::collections::HashMap<String, AgentLabel>;

/// Session id to display name and class, for every agent the app knows about.
///
/// Best-effort: a failure here costs labels, not data, so it degrades to the
/// raw keys rather than failing the whole read.
fn agent_labels() -> AgentLabels {
    wardian_core::db::get_all_agents()
        .map(|agents| {
            agents
                .into_iter()
                .map(|agent| {
                    (
                        agent.session_id,
                        AgentLabel {
                            name: agent.session_name,
                            class: agent.agent_class,
                        },
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The secondary line under a row's label.
///
/// Only agents have one worth showing. A provider or model row is already named
/// by the only attribute it has.
fn sublabel_for(key: &str, dimension: Dimension, labels: &AgentLabels) -> Option<String> {
    match dimension {
        Dimension::Agent => labels.get(key).and_then(|label| label.class.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_key_reads_as_unknown_rather_than_blank() {
        // `model` is nullable in the rollup and SQL groups nulls together, so
        // this row genuinely occurs: turns whose model was never stated.
        let labels = AgentLabels::new();
        assert_eq!(label_for("", Dimension::Model, &labels), "Unknown model");
        assert_eq!(label_for("", Dimension::Agent, &labels), "Unknown agent");
    }

    #[test]
    fn an_agent_key_resolves_to_its_name() {
        let labels = AgentLabels::from([(
            "uuid-1".to_string(),
            AgentLabel {
                name: "Scout".to_string(),
                class: Some("Coder".to_string()),
            },
        )]);
        assert_eq!(label_for("uuid-1", Dimension::Agent, &labels), "Scout");
        // The class rides along as the row's second line, so a grid of 40 rows
        // stays readable without a lookup elsewhere.
        assert_eq!(
            sublabel_for("uuid-1", Dimension::Agent, &labels).as_deref(),
            Some("Coder")
        );
        // Only agents have one; a provider or model row is already named by the
        // only attribute it has.
        assert_eq!(sublabel_for("codex", Dimension::Provider, &labels), None);
    }

    #[test]
    fn an_unknown_agent_falls_back_to_its_key() {
        // A deleted agent still has history. Dropping the row would make the
        // totals disagree with the summary they were computed from.
        let labels = AgentLabels::new();
        assert_eq!(label_for("uuid-9", Dimension::Agent, &labels), "uuid-9");
    }

    #[test]
    fn provider_and_model_keys_are_their_own_labels() {
        let labels = AgentLabels::from([(
            "codex".to_string(),
            AgentLabel {
                name: "wrong".to_string(),
                class: None,
            },
        )]);
        // The agent map must not leak into other dimensions: a provider named
        // the same as a session id would otherwise be relabelled.
        assert_eq!(label_for("codex", Dimension::Provider, &labels), "codex");
        assert_eq!(label_for("codex", Dimension::Model, &labels), "codex");
    }
}
