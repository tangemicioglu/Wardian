//! Pure scheduling math + persistence for automation schedule invokers. No Tauri, no app state.

use crate::models::{AutomationAssignments, ScheduleDefinition};
use chrono::{Datelike, TimeZone};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::models::AutomationSchedule;

/// Largest supported weekly recurrence interval.
///
/// Weekly scheduling currently searches forward by day, so keeping this
/// bounded prevents malformed or extreme persisted values from overflowing
/// the search window or monopolizing a scheduler tick. 520 weeks is roughly
/// ten years, which covers normal calendar scheduling while keeping the
/// projection bounded.
pub const MAX_WEEKLY_REPEAT_EVERY: u32 = 520;

/// Next future fire time in epoch-ms for `schedule`, or `None` if the schedule
/// can never fire again. Missed slots are skipped.
pub fn compute_next_run(schedule: &ScheduleDefinition, now_ms: u64) -> Option<u64> {
    match schedule.schedule_type.as_str() {
        "interval" => {
            let mins = schedule.interval_minutes.unwrap_or(0) as u64;
            if mins > 0 {
                Some(now_ms + mins * 60_000)
            } else {
                None
            }
        }
        "daily" => {
            let time_str = schedule.time_of_day.as_deref().unwrap_or("00:00");
            let parts: Vec<&str> = time_str.split(':').collect();
            if parts.len() != 2 {
                return None;
            }
            let hour: u32 = parts[0].parse().unwrap_or(0);
            let minute: u32 = parts[1].parse().unwrap_or(0);

            let now_local = chrono::Local::now();
            let today = now_local.date_naive();
            let target_time = chrono::NaiveTime::from_hms_opt(hour, minute, 0)?;
            let target_naive = today.and_time(target_time);
            let target_local = chrono::Local
                .from_local_datetime(&target_naive)
                .earliest()?;

            let target_ms = target_local.timestamp_millis() as u64;
            if target_ms > now_ms {
                Some(target_ms)
            } else {
                Some(target_ms + 86_400_000)
            }
        }
        "weekly" => {
            let time_str = schedule.time_of_day.as_deref().unwrap_or("00:00");
            let time_parts: Vec<&str> = time_str.split(':').collect();
            if time_parts.len() != 2 {
                return None;
            }
            let hour: u32 = time_parts[0].parse().unwrap_or(0);
            let minute: u32 = time_parts[1].parse().unwrap_or(0);

            let day_names = match &schedule.days_of_week {
                Some(d) if !d.is_empty() => d.clone(),
                _ => return None,
            };

            let day_map = |name: &str| -> Option<chrono::Weekday> {
                match name.to_lowercase().as_str() {
                    "mon" => Some(chrono::Weekday::Mon),
                    "tue" => Some(chrono::Weekday::Tue),
                    "wed" => Some(chrono::Weekday::Wed),
                    "thu" => Some(chrono::Weekday::Thu),
                    "fri" => Some(chrono::Weekday::Fri),
                    "sat" => Some(chrono::Weekday::Sat),
                    "sun" => Some(chrono::Weekday::Sun),
                    _ => None,
                }
            };

            if schedule.repeat_every > MAX_WEEKLY_REPEAT_EVERY {
                return None;
            }
            let repeat_weeks = schedule.repeat_every.max(1) as i64;
            let now_local = chrono::Local::now();
            let mut best: Option<u64> = None;

            let search_days = repeat_weeks.checked_mul(7)?.checked_add(7)? as u32;
            for day_name in &day_names {
                if let Some(target_day) = day_map(day_name) {
                    for offset in 0..search_days {
                        let candidate_date =
                            (now_local + chrono::Duration::days(offset as i64)).date_naive();
                        if candidate_date.weekday() == target_day {
                            if repeat_weeks > 1 {
                                let epoch = chrono::NaiveDate::from_ymd_opt(2000, 1, 3).unwrap();
                                let weeks_since = (candidate_date - epoch).num_weeks();
                                if weeks_since.rem_euclid(repeat_weeks) != 0 {
                                    continue;
                                }
                            }
                            let target_time = chrono::NaiveTime::from_hms_opt(hour, minute, 0)?;
                            let candidate_naive = candidate_date.and_time(target_time);
                            if let Some(candidate_local) = chrono::Local
                                .from_local_datetime(&candidate_naive)
                                .earliest()
                            {
                                let candidate_ms = candidate_local.timestamp_millis() as u64;
                                if candidate_ms > now_ms {
                                    best = Some(
                                        best.map_or(candidate_ms, |b: u64| b.min(candidate_ms)),
                                    );
                                    break;
                                }
                            }
                        }
                    }
                }
            }

            best
        }
        "monthly" => {
            let time_str = schedule.time_of_day.as_deref().unwrap_or("00:00");
            let time_parts: Vec<&str> = time_str.split(':').collect();
            if time_parts.len() != 2 {
                return None;
            }
            let hour: u32 = time_parts[0].parse().unwrap_or(0);
            let minute: u32 = time_parts[1].parse().unwrap_or(0);

            let target_days = match &schedule.days_of_month {
                Some(d) if !d.is_empty() => d.clone(),
                _ => return None,
            };

            let now_local = chrono::Local::now();
            let target_time = chrono::NaiveTime::from_hms_opt(hour, minute, 0)?;
            let mut best: Option<u64> = None;

            for month_offset in 0..3i32 {
                let candidate_month = now_local.month() as i32 + month_offset;
                let candidate_year = now_local.year() + (candidate_month - 1) / 12;
                let candidate_month_norm = ((candidate_month - 1) % 12 + 1) as u32;

                for &day in &target_days {
                    if let Some(candidate_date) =
                        chrono::NaiveDate::from_ymd_opt(candidate_year, candidate_month_norm, day)
                    {
                        let candidate_naive = candidate_date.and_time(target_time);
                        if let Some(candidate_local) = chrono::Local
                            .from_local_datetime(&candidate_naive)
                            .earliest()
                        {
                            let candidate_ms = candidate_local.timestamp_millis() as u64;
                            if candidate_ms > now_ms {
                                best =
                                    Some(best.map_or(candidate_ms, |b: u64| b.min(candidate_ms)));
                            }
                        }
                    }
                }

                if best.is_some() {
                    break;
                }
            }

            best
        }
        "specific_dates" => {
            let time_str = schedule.time_of_day.as_deref().unwrap_or("00:00");
            let time_parts: Vec<&str> = time_str.split(':').collect();
            let hour: u32 = time_parts.first().and_then(|p| p.parse().ok()).unwrap_or(0);
            let minute: u32 = time_parts.get(1).and_then(|p| p.parse().ok()).unwrap_or(0);

            let dates = match &schedule.specific_dates {
                Some(d) if !d.is_empty() => d.clone(),
                _ => return None,
            };

            let target_time = chrono::NaiveTime::from_hms_opt(hour, minute, 0)?;
            let mut best: Option<u64> = None;

            for date_str in &dates {
                if let Ok(date) = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
                    let candidate_naive = date.and_time(target_time);
                    if let Some(candidate_local) = chrono::Local
                        .from_local_datetime(&candidate_naive)
                        .earliest()
                    {
                        let candidate_ms = candidate_local.timestamp_millis() as u64;
                        if candidate_ms > now_ms {
                            best = Some(best.map_or(candidate_ms, |b: u64| b.min(candidate_ms)));
                        }
                    }
                }
            }

            best
        }
        "one_time" => {
            let run_at = schedule.run_at.as_deref().unwrap_or("");
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(run_at) {
                let ms = dt.timestamp_millis() as u64;
                if ms > now_ms {
                    Some(ms)
                } else {
                    None
                }
            } else if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(run_at, "%Y-%m-%dT%H:%M") {
                let local = chrono::Local.from_local_datetime(&dt).earliest()?;
                let ms = local.timestamp_millis() as u64;
                if ms > now_ms {
                    Some(ms)
                } else {
                    None
                }
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Validate a persisted schedule definition before it is handed to the scheduler.
///
/// The UI and CLI both construct this DTO, so this check deliberately validates
/// the serialized fields rather than relying on a particular caller's controls.
pub fn validate_schedule_definition(schedule: &ScheduleDefinition) -> Result<(), String> {
    match schedule.schedule_type.as_str() {
        "interval" => {
            if schedule.interval_minutes.unwrap_or(0) == 0 {
                return Err("interval schedules require --every greater than zero".to_string());
            }
        }
        "daily" => {
            validate_time_of_day(schedule.time_of_day.as_deref())?;
        }
        "weekly" => {
            validate_time_of_day(schedule.time_of_day.as_deref())?;
            let days = schedule
                .days_of_week
                .as_ref()
                .filter(|days| !days.is_empty())
                .ok_or_else(|| "weekly schedules require at least one day".to_string())?;
            for day in days {
                if !matches!(
                    day.to_ascii_lowercase().as_str(),
                    "mon" | "tue" | "wed" | "thu" | "fri" | "sat" | "sun"
                ) {
                    return Err(format!("invalid weekly day `{day}`"));
                }
            }
            if schedule.repeat_every == 0 {
                return Err("weekly schedules require repeat_every greater than zero".to_string());
            }
            if schedule.repeat_every > MAX_WEEKLY_REPEAT_EVERY {
                return Err(format!(
                    "weekly schedules require repeat_every no greater than {MAX_WEEKLY_REPEAT_EVERY}"
                ));
            }
        }
        "monthly" => {
            validate_time_of_day(schedule.time_of_day.as_deref())?;
            let days = schedule
                .days_of_month
                .as_ref()
                .filter(|days| !days.is_empty())
                .ok_or_else(|| "monthly schedules require at least one day".to_string())?;
            if let Some(day) = days.iter().find(|day| **day == 0 || **day > 31) {
                return Err(format!("invalid monthly day `{day}`; expected 1-31"));
            }
        }
        "specific_dates" => {
            validate_time_of_day(schedule.time_of_day.as_deref())?;
            let dates = schedule
                .specific_dates
                .as_ref()
                .filter(|dates| !dates.is_empty())
                .ok_or_else(|| "specific_dates schedules require at least one date".to_string())?;
            for date in dates {
                chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
                    .map_err(|_| format!("invalid specific date `{date}`; expected YYYY-MM-DD"))?;
            }
        }
        "one_time" => {
            let run_at = schedule
                .run_at
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| "one_time schedules require a run_at value".to_string())?;
            if chrono::DateTime::parse_from_rfc3339(run_at).is_err()
                && chrono::NaiveDateTime::parse_from_str(run_at, "%Y-%m-%dT%H:%M").is_err()
            {
                return Err(format!(
                    "invalid run_at `{run_at}`; expected RFC3339 or YYYY-MM-DDTHH:MM"
                ));
            }
        }
        other => {
            return Err(format!(
                "unsupported schedule type `{other}`; expected interval, daily, weekly, monthly, specific_dates, or one_time"
            ));
        }
    }

    match schedule.end_condition.as_str() {
        "never" => {}
        "on_date" => {
            let end_date = schedule
                .end_date
                .as_deref()
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| "on_date schedules require end_date".to_string())?;
            chrono::NaiveDate::parse_from_str(end_date, "%Y-%m-%d")
                .map_err(|_| format!("invalid end_date `{end_date}`; expected YYYY-MM-DD"))?;
        }
        "after_occurrences" => {
            if schedule.max_occurrences.unwrap_or(0) == 0 {
                return Err(
                    "after_occurrences schedules require max_occurrences greater than zero"
                        .to_string(),
                );
            }
        }
        other => {
            return Err(format!(
                "unsupported end condition `{other}`; expected never, on_date, or after_occurrences"
            ));
        }
    }

    Ok(())
}

fn validate_time_of_day(time: Option<&str>) -> Result<(), String> {
    let time = time
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "schedule requires a time_of_day value".to_string())?;
    let mut parts = time.split(':');
    let hour = parts
        .next()
        .and_then(|part| part.parse::<u32>().ok())
        .ok_or_else(|| format!("invalid time_of_day `{time}`; expected HH:MM"))?;
    let minute = parts
        .next()
        .and_then(|part| part.parse::<u32>().ok())
        .ok_or_else(|| format!("invalid time_of_day `{time}`; expected HH:MM"))?;
    if parts.next().is_some() || hour > 23 || minute > 59 {
        return Err(format!("invalid time_of_day `{time}`; expected HH:MM"));
    }
    Ok(())
}

/// Resolve and validate a schedule workspace to an absolute existing directory.
pub fn resolve_workspace_path(value: &str) -> Result<PathBuf, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("workspace is required".to_string());
    }
    let raw = Path::new(trimmed);
    let absolute = if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| format!("could not resolve current directory: {error}"))?
            .join(raw)
    };
    let canonical = absolute
        .canonicalize()
        .map_err(|error| format!("workspace is not an existing directory: {error}"))?;
    #[cfg(windows)]
    let canonical = {
        let value = canonical.to_string_lossy();
        value
            .strip_prefix(r"\\?\")
            .map(PathBuf::from)
            .unwrap_or(canonical)
    };
    if !canonical.is_dir() {
        return Err(format!(
            "workspace is not a directory: {}",
            canonical.display()
        ));
    }
    Ok(canonical)
}

#[derive(Serialize, Deserialize)]
struct ScheduleFile {
    #[serde(default = "default_schema")]
    schema: u8,
    #[serde(default)]
    schedules: Vec<AutomationSchedule>,
}

fn default_schema() -> u8 {
    1
}

/// Read all schedules. Missing or malformed file -> empty (logged to stderr), never panics.
pub fn load_schedules() -> Vec<AutomationSchedule> {
    let Some(path) = crate::paths::schedules_path() else {
        return Vec::new();
    };
    let Ok(content) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    match serde_json::from_str::<ScheduleFile>(&content) {
        Ok(file) => file.schedules,
        Err(err) => {
            eprintln!("[wardian-core] malformed schedules.json: {err}");
            Vec::new()
        }
    }
}

/// Write all schedules atomically (temp file + rename) so a crash mid-write cannot truncate.
pub fn save_schedules(schedules: &[AutomationSchedule]) -> std::io::Result<()> {
    let path = crate::paths::schedules_path()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no wardian home"))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = ScheduleFile {
        schema: 1,
        schedules: schedules.to_vec(),
    };
    let body = serde_json::to_string_pretty(&file)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, body)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// What the effect layer must launch for one firing schedule.
#[derive(Debug, Clone, PartialEq)]
pub struct FireRequest {
    pub schedule_id: String,
    pub blueprint_id: String,
    pub name: String,
    pub provider: Option<String>,
    pub workspace: Option<String>,
    pub input: serde_json::Value,
    pub bindings: HashMap<String, String>,
    pub assignments: AutomationAssignments,
}

fn is_expired(schedule: &AutomationSchedule, now_ms: u64) -> bool {
    match schedule.schedule.end_condition.as_str() {
        "after_occurrences" => schedule
            .schedule
            .max_occurrences
            .is_some_and(|max| schedule.schedule.occurrence_count >= max),
        "on_date" => schedule.schedule.end_date.as_ref().is_some_and(|date| {
            let Some(now) = chrono::DateTime::<chrono::Utc>::from_timestamp_millis(now_ms as i64)
            else {
                return false;
            };
            chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d")
                .map(|end| now.with_timezone(&chrono::Local).date_naive() > end)
                .unwrap_or(false)
        }),
        _ => false,
    }
}

fn fire_request(schedule: &AutomationSchedule) -> FireRequest {
    FireRequest {
        schedule_id: schedule.id.clone(),
        blueprint_id: schedule.blueprint_id.clone(),
        name: schedule.name.clone(),
        provider: schedule.provider.clone(),
        workspace: schedule.workspace.clone(),
        input: schedule.input.clone(),
        bindings: schedule.bindings.clone(),
        assignments: schedule.assignments.clone(),
    }
}

fn advance_after_fire(schedule: &mut AutomationSchedule, now_ms: u64) -> bool {
    schedule.schedule.occurrence_count = schedule.schedule.occurrence_count.saturating_add(1);
    schedule.last_run_status = Some("running".to_string());
    schedule.last_run_error = None;
    schedule.last_run_epoch_ms = Some(now_ms);

    if schedule.schedule.schedule_type == "one_time" || is_expired(schedule, now_ms) {
        return true;
    }

    schedule.next_run_epoch_ms = compute_next_run(&schedule.schedule, now_ms);
    schedule.next_run_epoch_ms.is_none() && schedule.schedule.schedule_type == "specific_dates"
}

/// Advance one tick. Mutates `schedules` and returns due fire requests.
pub fn plan_tick(schedules: &mut Vec<AutomationSchedule>, now_ms: u64) -> Vec<FireRequest> {
    let mut fire_requests = Vec::new();
    let mut remove_ids = Vec::new();

    for schedule in schedules.iter_mut() {
        if !schedule.schedule.active || schedule.is_paused {
            continue;
        }

        if is_expired(schedule, now_ms) {
            remove_ids.push(schedule.id.clone());
            continue;
        }

        let Some(next_run) = schedule.next_run_epoch_ms else {
            schedule.next_run_epoch_ms = compute_next_run(&schedule.schedule, now_ms);
            continue;
        };

        if next_run > now_ms {
            continue;
        }

        fire_requests.push(fire_request(schedule));
        if advance_after_fire(schedule, now_ms) {
            remove_ids.push(schedule.id.clone());
        }
    }

    if !remove_ids.is_empty() {
        schedules.retain(|schedule| !remove_ids.iter().any(|id| id == &schedule.id));
    }

    fire_requests
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ScheduleDefinition;

    fn sample_schedule(id: &str) -> crate::models::AutomationSchedule {
        crate::models::AutomationSchedule {
            id: id.into(),
            blueprint_id: "heartbeat".into(),
            name: "Heartbeat".into(),
            provider: None,
            workspace: None,
            input: serde_json::json!({}),
            bindings: std::collections::HashMap::new(),
            assignments: std::collections::HashMap::new(),
            schedule: ScheduleDefinition {
                schedule_type: "interval".into(),
                interval_minutes: Some(60),
                active: true,
                ..Default::default()
            },
            next_run_epoch_ms: None,
            paused_remaining_ms: None,
            is_paused: false,
            last_run_status: None,
            last_run_error: None,
            last_run_epoch_ms: None,
        }
    }

    fn s_vec(s: &crate::models::AutomationSchedule) -> Vec<crate::models::AutomationSchedule> {
        vec![s.clone()]
    }

    fn interval(mins: u32) -> ScheduleDefinition {
        ScheduleDefinition {
            schedule_type: "interval".into(),
            interval_minutes: Some(mins),
            active: true,
            ..Default::default()
        }
    }

    #[test]
    fn interval_projects_forward_from_now() {
        let next = compute_next_run(&interval(5), 1_000_000).unwrap();
        assert_eq!(next, 1_000_000 + 5 * 60_000);
    }

    #[test]
    fn skip_missed_never_returns_past_slot() {
        let next = compute_next_run(&interval(60), 10_000_000_000).unwrap();
        assert!(next > 10_000_000_000);
    }

    #[test]
    fn interval_zero_is_inert() {
        assert!(compute_next_run(&interval(0), 0).is_none());
    }

    #[test]
    fn compute_next_run_weekly_epoch_alignment() {
        use chrono::TimeZone;
        let schedule = ScheduleDefinition {
            schedule_type: "weekly".to_string(),
            time_of_day: Some("09:00".to_string()),
            days_of_week: Some(vec!["Mon".to_string(), "Wed".to_string()]),
            repeat_every: 2,
            ..Default::default()
        };
        let now = chrono::NaiveDate::from_ymd_opt(2024, 1, 1)
            .unwrap()
            .and_time(chrono::NaiveTime::from_hms_opt(8, 0, 0).unwrap());
        let now_ms = chrono::Local
            .from_local_datetime(&now)
            .earliest()
            .unwrap()
            .timestamp_millis() as u64;

        let next = compute_next_run(&schedule, now_ms);
        assert!(next.is_some());
        assert!(next.unwrap() > now_ms);
    }

    #[test]
    fn weekly_repeat_every_is_bounded_before_search() {
        let schedule = ScheduleDefinition {
            schedule_type: "weekly".to_string(),
            time_of_day: Some("09:00".to_string()),
            days_of_week: Some(vec!["Mon".to_string()]),
            repeat_every: MAX_WEEKLY_REPEAT_EVERY,
            end_condition: "never".to_string(),
            ..Default::default()
        };
        assert!(compute_next_run(&schedule, 0).is_some());
        validate_schedule_definition(&schedule).unwrap();

        let out_of_bounds = ScheduleDefinition {
            repeat_every: MAX_WEEKLY_REPEAT_EVERY + 1,
            ..schedule
        };
        assert!(compute_next_run(&out_of_bounds, 0).is_none());
        let error = validate_schedule_definition(&out_of_bounds).unwrap_err();
        assert!(error.contains("no greater than 520"));
    }

    #[test]
    fn save_then_load_round_trips() {
        let _guard = crate::tests::env_lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("WARDIAN_HOME", dir.path());
        let scheds = vec![sample_schedule("s1")];
        save_schedules(&scheds).unwrap();
        let loaded = load_schedules();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "s1");
        std::env::remove_var("WARDIAN_HOME");
    }

    #[test]
    fn load_missing_file_is_empty() {
        let _guard = crate::tests::env_lock();
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("WARDIAN_HOME", dir.path());
        assert!(load_schedules().is_empty());
        std::env::remove_var("WARDIAN_HOME");
    }

    #[test]
    fn due_active_schedule_fires_and_advances() {
        let mut s = sample_schedule("s1");
        s.next_run_epoch_ms = Some(500);
        let mut v = s_vec(&s);
        let fires = plan_tick(&mut v, 1000);
        assert_eq!(fires.len(), 1);
        assert_eq!(fires[0].blueprint_id, "heartbeat");
        assert!(v[0].next_run_epoch_ms.is_some_and(|next| next > 1000));
    }

    #[test]
    fn paused_schedule_does_not_fire() {
        let mut s = sample_schedule("s1");
        s.is_paused = true;
        s.next_run_epoch_ms = Some(500);
        let mut v = vec![s];
        assert!(plan_tick(&mut v, 1000).is_empty());
    }

    #[test]
    fn unset_next_run_is_computed_not_fired() {
        let mut v = vec![sample_schedule("s1")];
        let fires = plan_tick(&mut v, 1000);
        assert!(fires.is_empty());
        assert!(v[0].next_run_epoch_ms.is_some());
    }

    #[test]
    fn one_time_is_removed_after_firing() {
        let mut s = sample_schedule("s1");
        s.schedule.schedule_type = "one_time".into();
        s.schedule.interval_minutes = None;
        s.next_run_epoch_ms = Some(500);
        let mut v = vec![s];
        let fires = plan_tick(&mut v, 1000);
        assert_eq!(fires.len(), 1);
        assert!(
            v.is_empty(),
            "one_time schedule should be removed after firing"
        );
    }

    #[test]
    fn after_occurrences_expiry_removes_without_firing() {
        let mut s = sample_schedule("s1");
        s.schedule.end_condition = "after_occurrences".into();
        s.schedule.max_occurrences = Some(2);
        s.schedule.occurrence_count = 2;
        s.next_run_epoch_ms = Some(500);
        let mut v = vec![s];
        let fires = plan_tick(&mut v, 1000);
        assert!(fires.is_empty());
        assert!(v.is_empty());
    }

    #[test]
    fn validates_extended_cadence_and_end_fields() {
        let monthly = ScheduleDefinition {
            schedule_type: "monthly".into(),
            time_of_day: Some("09:30".into()),
            days_of_month: Some(vec![1, 15]),
            end_condition: "after_occurrences".into(),
            max_occurrences: Some(4),
            active: true,
            ..Default::default()
        };
        validate_schedule_definition(&monthly).unwrap();

        let invalid = ScheduleDefinition {
            schedule_type: "weekly".into(),
            time_of_day: Some("25:00".into()),
            days_of_week: Some(vec!["Mon".into()]),
            repeat_every: 1,
            active: true,
            ..Default::default()
        };
        assert!(validate_schedule_definition(&invalid).is_err());
    }

    #[test]
    fn resolves_only_existing_directories_as_workspaces() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("not-a-directory");
        std::fs::write(&file, "x").unwrap();

        assert!(resolve_workspace_path(&dir.path().to_string_lossy()).is_ok());
        assert!(resolve_workspace_path(&file.to_string_lossy()).is_err());
        assert!(resolve_workspace_path("definitely-missing-workspace").is_err());
    }
}
