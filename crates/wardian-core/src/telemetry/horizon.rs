//! Time horizons, and the rule for turning one into a query window.
//!
//! This lives in the core rather than in either caller because the flooring
//! rule below is a correctness property, not a presentation choice. The app and
//! the CLI both answer "the last 24 hours", and if they resolved that
//! independently they could disagree about which hour a figure belongs to —
//! a divergence that shows up as two surfaces quoting different numbers for the
//! same question, with nothing to say which is right.

use chrono::{DateTime, Datelike, Duration, Local, TimeZone, Timelike, Utc};
use serde::{Deserialize, Serialize};

/// How far back a caller is asking about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Horizon {
    /// Since local midnight. The only horizon anchored to a wall clock rather
    /// than to a rolling offset, because "today" means the user's day.
    Today,
    Day,
    Week,
    Month,
    /// Everything the store holds.
    All,
}

impl Horizon {
    pub fn as_str(self) -> &'static str {
        match self {
            Horizon::Today => "today",
            Horizon::Day => "day",
            Horizon::Week => "week",
            Horizon::Month => "month",
            Horizon::All => "all",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "today" => Some(Horizon::Today),
            "day" | "24h" => Some(Horizon::Day),
            "week" | "7d" => Some(Horizon::Week),
            "month" | "30d" => Some(Horizon::Month),
            "all" => Some(Horizon::All),
            _ => None,
        }
    }
}

/// A resolved half-open window `[from, to)` in UTC RFC 3339.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HorizonWindow {
    pub from: String,
    pub to: String,
    /// True when `from` was moved back to an hour boundary, so a caller can say
    /// the window is approximate rather than presenting it as exact.
    pub from_floored: bool,
}

/// Resolve a horizon against a given "now".
///
/// `from` is floored to the hour because a bucket is only included when
/// `bucket_start >= from`; leaving a mid-hour `from` in place would discard the
/// entire hour it falls in. For the 24h horizon that means dropping up to 59
/// minutes of the most recent — and most looked-at — activity, silently.
/// Flooring instead over-includes by less than an hour, and says that it did.
///
/// `to` is left exactly at `now`. It bounds nothing that exists yet, since no
/// bucket can start in the future.
pub fn resolve_horizon(horizon: Horizon, now: DateTime<Utc>) -> HorizonWindow {
    let raw_from = match horizon {
        Horizon::Today => local_midnight_before(now),
        Horizon::Day => now - Duration::days(1),
        Horizon::Week => now - Duration::days(7),
        Horizon::Month => now - Duration::days(30),
        // Comfortably before any provider log this store can read, and a fixed
        // instant rather than an empty string so every caller compares the same
        // way.
        Horizon::All => Utc.with_ymd_and_hms(1970, 1, 1, 0, 0, 0).unwrap(),
    };

    let from = floor_to_hour(raw_from);
    HorizonWindow {
        from: format_instant(from),
        to: format_instant(now),
        from_floored: from != raw_from,
    }
}

fn format_instant(instant: DateTime<Utc>) -> String {
    instant.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn floor_to_hour(instant: DateTime<Utc>) -> DateTime<Utc> {
    instant
        .with_minute(0)
        .and_then(|value| value.with_second(0))
        .and_then(|value| value.with_nanosecond(0))
        .unwrap_or(instant)
}

/// The most recent local midnight, expressed in UTC.
///
/// Falls back to a rolling day when the local date is ambiguous — which happens
/// for real, on the day a zone's clocks shift across midnight — rather than
/// picking one of the two candidate instants arbitrarily.
fn local_midnight_before(now: DateTime<Utc>) -> DateTime<Utc> {
    let local = now.with_timezone(&Local);
    Local
        .with_ymd_and_hms(local.year(), local.month(), local.day(), 0, 0, 0)
        .single()
        .map(|midnight| midnight.with_timezone(&Utc))
        .unwrap_or_else(|| now - Duration::days(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn horizon_names_round_trip_and_reject_anything_else() {
        for horizon in [
            Horizon::Today,
            Horizon::Day,
            Horizon::Week,
            Horizon::Month,
            Horizon::All,
        ] {
            assert_eq!(Horizon::parse(horizon.as_str()), Some(horizon));
        }
        // The spellings a surface is likely to use.
        assert_eq!(Horizon::parse("24h"), Some(Horizon::Day));
        assert_eq!(Horizon::parse("7d"), Some(Horizon::Week));
        assert_eq!(Horizon::parse("30d"), Some(Horizon::Month));
        assert_eq!(Horizon::parse("fortnight"), None);
    }

    #[test]
    fn a_rolling_horizon_floors_its_start_to_the_hour() {
        // The failure this prevents: `bucket_start >= from` with a mid-hour
        // `from` drops the whole hour it lands in. For the 24h window that is up
        // to 59 minutes of the most recent activity, silently missing from the
        // figure a user is most likely to check.
        let window = resolve_horizon(Horizon::Day, at("2026-08-13T18:42:49.628Z"));
        assert_eq!(window.from, "2026-08-12T18:00:00.000Z");
        assert_eq!(window.to, "2026-08-13T18:42:49.628Z");
        assert!(window.from_floored);
    }

    #[test]
    fn an_already_aligned_start_is_not_reported_as_floored() {
        let window = resolve_horizon(Horizon::Day, at("2026-08-13T18:00:00.000Z"));
        assert_eq!(window.from, "2026-08-12T18:00:00.000Z");
        assert!(!window.from_floored);
    }

    #[test]
    fn horizons_are_ordered_by_length_and_share_an_end() {
        let now = at("2026-08-13T18:42:49.628Z");
        let day = resolve_horizon(Horizon::Day, now);
        let week = resolve_horizon(Horizon::Week, now);
        let month = resolve_horizon(Horizon::Month, now);
        let all = resolve_horizon(Horizon::All, now);

        assert!(all.from < month.from);
        assert!(month.from < week.from);
        assert!(week.from < day.from);
        // Every window ends at the same instant, so two horizons are directly
        // comparable rather than each being "as of" a different moment.
        for window in [&week, &month, &all] {
            assert_eq!(window.to, day.to);
        }
    }

    #[test]
    fn the_all_horizon_starts_before_any_provider_log() {
        let window = resolve_horizon(Horizon::All, at("2026-08-13T18:42:49.628Z"));
        assert_eq!(window.from, "1970-01-01T00:00:00.000Z");
    }

    #[test]
    fn today_starts_at_a_local_midnight_and_never_after_now() {
        // Anchored to the user's wall clock, so this asserts the invariants that
        // hold in every zone rather than a fixed instant that would only hold in
        // whichever zone the test happened to run in.
        let now = at("2026-08-13T18:42:49.628Z");
        let window = resolve_horizon(Horizon::Today, now);
        let from = at(&window.from);

        assert!(from <= now, "today cannot start in the future");
        // A local day is at most 26 hours from any UTC instant within it once
        // the offset range and the hour flooring are accounted for.
        assert!(now - from < Duration::hours(27));
        assert_eq!(from, floor_to_hour(from));
    }
}
