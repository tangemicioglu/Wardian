//! Gap clustering for providers that report event timestamps but no durations.
//!
//! Codex, claude, and antigravity emit timestamped events without telling us
//! how long any turn took. Clustering recovers approximate spans from those
//! timestamps. OpenCode reports real start and end times and must never reach
//! this module — its intervals are measured, and running them through the
//! clusterer would replace a fact with an estimate.
//!
//! The thresholds are carried over from the prior activity artifact so figures
//! stay comparable with the analysis that motivated this work.

use crate::telemetry::models::{ActivityMethod, IntervalFact};
use chrono::{DateTime, Utc};

/// Events further apart than this begin a new interval.
pub const ACTIVE_GAP_THRESHOLD_MS: i64 = 12 * 60 * 1000;

/// Duration credited to an interval containing a single event, which would
/// otherwise contribute nothing despite being real work.
pub const ACTIVE_SINGLETON_MS: i64 = 40 * 1000;

/// Cluster event timestamps into active intervals.
///
/// Input need not be sorted; out-of-order lines are common when a log is read
/// in byte-range deltas that straddle a writer flush.
pub fn cluster_events(
    session_id: &str,
    provider: &str,
    event_times: &[String],
    method: ActivityMethod,
) -> Vec<IntervalFact> {
    let mut stamps: Vec<DateTime<Utc>> = event_times
        .iter()
        .filter_map(|raw| parse_timestamp(raw))
        .collect();
    if stamps.is_empty() {
        return Vec::new();
    }
    stamps.sort_unstable();

    let mut intervals = Vec::new();
    let mut start = stamps[0];
    let mut end = stamps[0];
    let mut count: i64 = 1;

    for stamp in stamps.iter().skip(1) {
        let gap = (*stamp - end).num_milliseconds();
        if gap > ACTIVE_GAP_THRESHOLD_MS {
            intervals.push(build_interval(session_id, provider, start, end, count, method));
            start = *stamp;
            end = *stamp;
            count = 1;
        } else {
            end = *stamp;
            count += 1;
        }
    }
    intervals.push(build_interval(session_id, provider, start, end, count, method));
    intervals
}

fn build_interval(
    session_id: &str,
    provider: &str,
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    count: i64,
    method: ActivityMethod,
) -> IntervalFact {
    // A lone event has no measurable span; crediting it a fixed slice keeps it
    // from disappearing entirely.
    let credited_end = if start == end {
        start + chrono::Duration::milliseconds(ACTIVE_SINGLETON_MS)
    } else {
        end
    };
    IntervalFact {
        session_id: session_id.to_string(),
        provider: provider.to_string(),
        started_at: format_timestamp(start),
        ended_at: format_timestamp(credited_end),
        // Kept apart from `ended_at` because the singleton credit pushes that
        // forward by 40s. Resuming from the credited end would measure the next
        // gap from a moment at which nothing happened, so an event just over the
        // threshold from the real one would land just under it from the credited
        // one — and the same log would cluster differently depending on when
        // ingest ran.
        last_event_at: format_timestamp(end),
        event_count: count,
        method,
    }
}

/// Parse an RFC 3339 timestamp into UTC.
pub fn parse_timestamp(raw: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(raw.trim())
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

/// Render a timestamp in the millisecond-precision UTC form used throughout the
/// store, so lexicographic ordering matches chronological ordering.
pub fn format_timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

/// Convert epoch milliseconds, as used by opencode, into store form.
pub fn format_epoch_ms(epoch_ms: i64) -> Option<String> {
    DateTime::from_timestamp_millis(epoch_ms).map(format_timestamp)
}

/// Duration of an interval in milliseconds, or 0 if it is malformed.
pub fn interval_duration_ms(interval: &IntervalFact) -> i64 {
    match (
        parse_timestamp(&interval.started_at),
        parse_timestamp(&interval.ended_at),
    ) {
        (Some(start), Some(end)) => (end - start).num_milliseconds().max(0),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(minute: i64) -> String {
        format_timestamp(DateTime::from_timestamp_millis(minute * 60 * 1000).unwrap())
    }

    #[test]
    fn empty_input_yields_no_intervals() {
        assert!(cluster_events("s", "codex", &[], ActivityMethod::Clustered).is_empty());
    }

    #[test]
    fn close_events_form_one_interval() {
        let events = vec![at(0), at(5), at(10)];
        let intervals = cluster_events("s", "codex", &events, ActivityMethod::Clustered);
        assert_eq!(intervals.len(), 1);
        assert_eq!(intervals[0].event_count, 3);
        assert_eq!(interval_duration_ms(&intervals[0]), 10 * 60 * 1000);
    }

    #[test]
    fn a_gap_past_the_threshold_splits() {
        // 13 minutes exceeds the 12 minute threshold.
        let events = vec![at(0), at(5), at(18)];
        let intervals = cluster_events("s", "codex", &events, ActivityMethod::Clustered);
        assert_eq!(intervals.len(), 2);
        assert_eq!(intervals[0].event_count, 2);
        assert_eq!(intervals[1].event_count, 1);
    }

    #[test]
    fn a_gap_exactly_at_the_threshold_does_not_split() {
        let events = vec![at(0), at(12)];
        let intervals = cluster_events("s", "codex", &events, ActivityMethod::Clustered);
        assert_eq!(intervals.len(), 1);
    }

    #[test]
    fn singleton_gets_the_fixed_credit() {
        let intervals = cluster_events("s", "codex", &[at(0)], ActivityMethod::Clustered);
        assert_eq!(intervals.len(), 1);
        assert_eq!(interval_duration_ms(&intervals[0]), ACTIVE_SINGLETON_MS);
    }

    #[test]
    fn unsorted_input_is_ordered_before_clustering() {
        let events = vec![at(10), at(0), at(5)];
        let intervals = cluster_events("s", "codex", &events, ActivityMethod::Clustered);
        assert_eq!(intervals.len(), 1);
        assert_eq!(intervals[0].started_at, at(0));
        assert_eq!(interval_duration_ms(&intervals[0]), 10 * 60 * 1000);
    }

    #[test]
    fn unparseable_timestamps_are_dropped_not_fatal() {
        let events = vec!["not a timestamp".to_string(), at(0), at(3)];
        let intervals = cluster_events("s", "codex", &events, ActivityMethod::Clustered);
        assert_eq!(intervals.len(), 1);
        assert_eq!(intervals[0].event_count, 2);
    }

    #[test]
    fn method_is_carried_through() {
        let intervals = cluster_events("s", "antigravity", &[at(0)], ActivityMethod::Decoded);
        assert_eq!(intervals[0].method, ActivityMethod::Decoded);
    }

    #[test]
    fn epoch_ms_converts_to_store_form() {
        // The opencode sample timestamp from the source investigation.
        assert_eq!(
            format_epoch_ms(1786644338865).as_deref(),
            Some("2026-08-13T18:05:38.865Z")
        );
    }
}
