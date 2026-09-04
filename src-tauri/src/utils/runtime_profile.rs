//! Opt-in, low-overhead accounting for Wardian runtime hot paths.
//!
//! Set `WARDIAN_RUNTIME_PROFILE=1` before starting Wardian to write interval
//! deltas to `wardian_debug.log`. The profiler records only aggregate counts,
//! bytes, wall time, and current-thread CPU time; it never records agent IDs,
//! paths, terminal contents, or provider payloads.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const DEFAULT_REPORT_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug)]
#[repr(usize)]
pub(crate) enum RuntimeMetric {
    PtyRead,
    PtyPostprocess,
    TerminalBrokerOutput,
    TerminalRegister,
    TerminalUpdate,
    TerminalViewport,
    TerminalSnapshot,
    TerminalReadEvents,
    TerminalAckEvents,
    ProviderLogRead,
    CodexWatcherPoll,
    PiWatcherPoll,
    ClaudeWatcherPoll,
    ClaudeHookPoll,
    AntigravityWatcherPoll,
    AntigravityLatestStep,
    AntigravityMessageScan,
    TelemetryIngestDiscover,
    TelemetryIngestPass,
    TelemetryFleetQuery,
    TelemetryMatrixQuery,
    InboxApprovalScan,
    InboxTerminalScan,
    AppMetrics,
    MetricsTick,
}

impl RuntimeMetric {
    const COUNT: usize = 25;
    const ALL: [Self; Self::COUNT] = [
        Self::PtyRead,
        Self::PtyPostprocess,
        Self::TerminalBrokerOutput,
        Self::TerminalRegister,
        Self::TerminalUpdate,
        Self::TerminalViewport,
        Self::TerminalSnapshot,
        Self::TerminalReadEvents,
        Self::TerminalAckEvents,
        Self::ProviderLogRead,
        Self::CodexWatcherPoll,
        Self::PiWatcherPoll,
        Self::ClaudeWatcherPoll,
        Self::ClaudeHookPoll,
        Self::AntigravityWatcherPoll,
        Self::AntigravityLatestStep,
        Self::AntigravityMessageScan,
        Self::TelemetryIngestDiscover,
        Self::TelemetryIngestPass,
        Self::TelemetryFleetQuery,
        Self::TelemetryMatrixQuery,
        Self::InboxApprovalScan,
        Self::InboxTerminalScan,
        Self::AppMetrics,
        Self::MetricsTick,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::PtyRead => "pty_read",
            Self::PtyPostprocess => "pty_postprocess",
            Self::TerminalBrokerOutput => "terminal_broker_output",
            Self::TerminalRegister => "terminal_register",
            Self::TerminalUpdate => "terminal_update",
            Self::TerminalViewport => "terminal_viewport",
            Self::TerminalSnapshot => "terminal_snapshot",
            Self::TerminalReadEvents => "terminal_read_events",
            Self::TerminalAckEvents => "terminal_ack_events",
            Self::ProviderLogRead => "provider_log_read",
            Self::CodexWatcherPoll => "codex_watcher_poll",
            Self::PiWatcherPoll => "pi_watcher_poll",
            Self::ClaudeWatcherPoll => "claude_watcher_poll",
            Self::ClaudeHookPoll => "claude_hook_poll",
            Self::AntigravityWatcherPoll => "antigravity_watcher_poll",
            Self::AntigravityLatestStep => "antigravity_latest_step",
            Self::AntigravityMessageScan => "antigravity_message_scan",
            Self::TelemetryIngestDiscover => "telemetry_ingest_discover",
            Self::TelemetryIngestPass => "telemetry_ingest_pass",
            Self::TelemetryFleetQuery => "telemetry_fleet_query",
            Self::TelemetryMatrixQuery => "telemetry_matrix_query",
            Self::InboxApprovalScan => "inbox_approval_scan",
            Self::InboxTerminalScan => "inbox_terminal_scan",
            Self::AppMetrics => "app_metrics",
            Self::MetricsTick => "metrics_tick",
        }
    }
}

struct MetricCounters {
    calls: AtomicU64,
    units: AtomicU64,
    wall_ns: AtomicU64,
    cpu_ns: AtomicU64,
}

impl MetricCounters {
    const fn new() -> Self {
        Self {
            calls: AtomicU64::new(0),
            units: AtomicU64::new(0),
            wall_ns: AtomicU64::new(0),
            cpu_ns: AtomicU64::new(0),
        }
    }

    fn record(&self, units: u64, wall_ns: u64, cpu_ns: u64) {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.units.fetch_add(units, Ordering::Relaxed);
        self.wall_ns.fetch_add(wall_ns, Ordering::Relaxed);
        self.cpu_ns.fetch_add(cpu_ns, Ordering::Relaxed);
    }

    fn take(&self) -> MetricSample {
        MetricSample {
            calls: self.calls.swap(0, Ordering::Relaxed),
            units: self.units.swap(0, Ordering::Relaxed),
            wall_ns: self.wall_ns.swap(0, Ordering::Relaxed),
            cpu_ns: self.cpu_ns.swap(0, Ordering::Relaxed),
        }
    }
}

static ENABLED: OnceLock<bool> = OnceLock::new();
static REPORTER_STARTED: AtomicBool = AtomicBool::new(false);
static COUNTERS: [MetricCounters; RuntimeMetric::COUNT] =
    [const { MetricCounters::new() }; RuntimeMetric::COUNT];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct MetricSample {
    calls: u64,
    units: u64,
    wall_ns: u64,
    cpu_ns: u64,
}

impl MetricSample {
    fn is_empty(self) -> bool {
        self.calls == 0 && self.units == 0 && self.wall_ns == 0 && self.cpu_ns == 0
    }
}

fn enabled() -> bool {
    *ENABLED.get_or_init(|| {
        std::env::var("WARDIAN_RUNTIME_PROFILE")
            .ok()
            .is_some_and(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes"
                )
            })
    })
}

fn report_interval() -> Duration {
    std::env::var("WARDIAN_RUNTIME_PROFILE_INTERVAL_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(|seconds| Duration::from_secs(seconds.clamp(1, 3_600)))
        .unwrap_or(DEFAULT_REPORT_INTERVAL)
}

pub(crate) fn record_event(metric: RuntimeMetric, units: u64) {
    if enabled() {
        COUNTERS[metric as usize].record(units, 0, 0);
    }
}

pub(crate) fn record_wall_time(metric: RuntimeMetric, units: u64, elapsed: Duration) {
    if enabled() {
        COUNTERS[metric as usize].record(units, duration_ns(elapsed), 0);
    }
}

#[must_use]
pub(crate) struct RuntimeProfileSpan {
    metric: RuntimeMetric,
    started: Option<Instant>,
    cpu_started_ns: Option<u64>,
}

impl RuntimeProfileSpan {
    pub(crate) fn start(metric: RuntimeMetric) -> Self {
        if !enabled() {
            return Self {
                metric,
                started: None,
                cpu_started_ns: None,
            };
        }
        Self {
            metric,
            started: Some(Instant::now()),
            cpu_started_ns: current_thread_cpu_ns(),
        }
    }

    pub(crate) fn wall(metric: RuntimeMetric) -> Self {
        if !enabled() {
            return Self {
                metric,
                started: None,
                cpu_started_ns: None,
            };
        }
        Self {
            metric,
            started: Some(Instant::now()),
            cpu_started_ns: None,
        }
    }

    pub(crate) fn finish(mut self, units: u64) {
        self.record(units);
    }

    fn record(&mut self, units: u64) {
        let Some(started) = self.started.take() else {
            return;
        };
        let cpu_ns = self
            .cpu_started_ns
            .take()
            .zip(current_thread_cpu_ns())
            .map(|(before, after)| after.saturating_sub(before))
            .unwrap_or(0);
        COUNTERS[self.metric as usize].record(units, duration_ns(started.elapsed()), cpu_ns);
    }
}

impl Drop for RuntimeProfileSpan {
    fn drop(&mut self) {
        self.record(0);
    }
}

pub(crate) fn start_reporter() {
    if !enabled() || REPORTER_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let interval = report_interval();
    let _ = std::thread::Builder::new()
        .name("wardian-runtime-profiler".to_string())
        .spawn(move || loop {
            std::thread::sleep(interval);
            let metrics = take_metrics_json();
            crate::utils::logging::log_debug(&format!(
                "[Wardian profile] {}",
                serde_json::json!({
                    "interval_ms": interval.as_millis(),
                    "metrics": metrics,
                })
            ));
        });
}

fn take_metrics_json() -> serde_json::Map<String, serde_json::Value> {
    RuntimeMetric::ALL
        .into_iter()
        .filter_map(|metric| {
            let sample = COUNTERS[metric as usize].take();
            (!sample.is_empty()).then(|| {
                (
                    metric.name().to_string(),
                    serde_json::json!({
                        "calls": sample.calls,
                        "units": sample.units,
                        "wall_ms": ns_to_ms(sample.wall_ns),
                        "cpu_ms": ns_to_ms(sample.cpu_ns),
                    }),
                )
            })
        })
        .collect()
}

fn duration_ns(duration: Duration) -> u64 {
    duration.as_nanos().min(u64::MAX as u128) as u64
}

fn ns_to_ms(nanoseconds: u64) -> f64 {
    nanoseconds as f64 / 1_000_000.0
}

#[cfg(windows)]
fn current_thread_cpu_ns() -> Option<u64> {
    use std::mem::MaybeUninit;
    use winapi::shared::minwindef::FILETIME;
    use winapi::um::processthreadsapi::{GetCurrentThread, GetThreadTimes};

    let mut created = MaybeUninit::<FILETIME>::uninit();
    let mut exited = MaybeUninit::<FILETIME>::uninit();
    let mut kernel = MaybeUninit::<FILETIME>::uninit();
    let mut user = MaybeUninit::<FILETIME>::uninit();
    let succeeded = unsafe {
        GetThreadTimes(
            GetCurrentThread(),
            created.as_mut_ptr(),
            exited.as_mut_ptr(),
            kernel.as_mut_ptr(),
            user.as_mut_ptr(),
        )
    };
    if succeeded == 0 {
        return None;
    }
    let kernel = unsafe { kernel.assume_init() };
    let user = unsafe { user.assume_init() };
    let ticks_100ns = filetime_ticks(kernel).saturating_add(filetime_ticks(user));
    Some(ticks_100ns.saturating_mul(100))
}

#[cfg(windows)]
fn filetime_ticks(value: winapi::shared::minwindef::FILETIME) -> u64 {
    (u64::from(value.dwHighDateTime) << 32) | u64::from(value.dwLowDateTime)
}

#[cfg(not(windows))]
fn current_thread_cpu_ns() -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metric_names_are_unique_and_complete() {
        let names = RuntimeMetric::ALL
            .into_iter()
            .map(RuntimeMetric::name)
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(names.len(), RuntimeMetric::COUNT);
    }

    #[test]
    fn counters_return_interval_deltas() {
        let counters = MetricCounters::new();
        counters.record(4_096, 2_000_000, 1_000_000);
        counters.record(512, 3_000_000, 2_000_000);

        assert_eq!(
            counters.take(),
            MetricSample {
                calls: 2,
                units: 4_608,
                wall_ns: 5_000_000,
                cpu_ns: 3_000_000,
            }
        );
        assert_eq!(counters.take(), MetricSample::default());
    }
}
