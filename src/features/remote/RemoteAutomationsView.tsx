import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  AlertTriangle,
  CheckCircle2,
  ChevronRight,
  Clock3,
  LoaderCircle,
  PauseCircle,
  RefreshCw,
  X,
  XCircle,
} from "lucide-react";
import type {
  RemoteAutomationMonitorRun,
  RemoteAutomationMonitorSchedule,
  RemoteAutomationMonitorSnapshot,
} from "../../types";
import { formatAutomationTime } from "../automations/monitor/automationTime";
import { automationAttention } from "../automations/monitor/attentionModel";
import { scheduleSummaryLabel } from "../automations/monitor/scheduleStatus";
import { remoteClient, RemoteRequestError } from "./remoteClient";

type Filter = "overview" | "attention" | "soon" | "history";
type Detail = { kind: "run"; value: RemoteAutomationMonitorRun } | { kind: "schedule"; value: RemoteAutomationMonitorSchedule };
type PageKind = "active" | "recent" | "schedules";
type RetainedPageIds = {
  active: Set<string>;
  recent: Set<string>;
  schedules: Set<string>;
};

const filters: ReadonlyArray<{ id: Filter; label: string }> = [
  { id: "overview", label: "Overview" },
  { id: "attention", label: "Attention" },
  { id: "soon", label: "Soon" },
  { id: "history", label: "History" },
];
const focusRing = "focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-[var(--color-wardian-accent)]";

const runTime = (run: RemoteAutomationMonitorRun) => run.updated_at ?? run.completed_at ?? run.started_at;
function uniqueBy<T>(left: T[], right: T[], key: (value: T) => string) {
  const merged = new Map(left.map((value) => [key(value), value]));
  for (const value of right) merged.set(key(value), value);
  return [...merged.values()];
}

function reconcileRefreshedItems<T>(
  fresh: T[],
  current: T[],
  retainedIds: Set<string>,
  key: (value: T) => string,
) {
  const reconciled = new Map(fresh.map((value) => [key(value), value]));
  for (const value of current) {
    const id = key(value);
    if (retainedIds.has(id) && !reconciled.has(id)) reconciled.set(id, value);
  }
  return [...reconciled.values()];
}

function reconcileRefresh(
  fresh: RemoteAutomationMonitorSnapshot,
  current: RemoteAutomationMonitorSnapshot,
  retained: RetainedPageIds,
): RemoteAutomationMonitorSnapshot {
  const retainedActive = retained.active.size > 0;
  const retainedRecent = retained.recent.size > 0;
  const retainedSchedules = retained.schedules.size > 0;
  return {
    ...fresh,
    active_runs: reconcileRefreshedItems(fresh.active_runs, current.active_runs, retained.active, (run) => run.run_id),
    active_runs_truncated: retainedActive ? current.active_runs_truncated : fresh.active_runs_truncated,
    active_runs_next_offset: retainedActive ? current.active_runs_next_offset : fresh.active_runs_next_offset,
    recent_runs: reconcileRefreshedItems(fresh.recent_runs, current.recent_runs, retained.recent, (run) => run.run_id),
    recent_runs_truncated: retainedRecent ? current.recent_runs_truncated : fresh.recent_runs_truncated,
    recent_runs_next_offset: retainedRecent ? current.recent_runs_next_offset : fresh.recent_runs_next_offset,
    schedules: reconcileRefreshedItems(fresh.schedules, current.schedules, retained.schedules, (schedule) => schedule.id),
    schedules_truncated: retainedSchedules ? current.schedules_truncated : fresh.schedules_truncated,
    schedules_next_offset: retainedSchedules ? current.schedules_next_offset : fresh.schedules_next_offset,
  };
}

function relativeTime(value: string | number | null) {
  if (value === null) return "Unknown time";
  const timestamp = typeof value === "number" ? value : Date.parse(value);
  if (!Number.isFinite(timestamp)) return "Unknown time";
  const minutes = Math.round((timestamp - Date.now()) / 60_000);
  const formatter = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });
  if (Math.abs(minutes) < 60) return formatter.format(minutes, "minute");
  const hours = Math.round(minutes / 60);
  if (Math.abs(hours) < 24) return formatter.format(hours, "hour");
  return formatter.format(Math.round(hours / 24), "day");
}

function attentionItems(snapshot: RemoteAutomationMonitorSnapshot) {
  const runs = [...snapshot.active_runs, ...snapshot.recent_runs];
  const attention = automationAttention(runs, snapshot.schedules);
  return [
    ...runs.filter((run) => attention.runIds.has(run.run_id)).map((value) => ({ kind: "run" as const, value })),
    ...snapshot.schedules.filter((schedule) => attention.scheduleIds.has(schedule.id)).map((value) => ({ kind: "schedule" as const, value })),
  ];
}

export const RemoteAutomationsView: React.FC = () => {
  const [snapshot, setSnapshot] = useState<RemoteAutomationMonitorSnapshot | null>(null);
  const [filter, setFilter] = useState<Filter>("overview");
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [loadingPage, setLoadingPage] = useState<PageKind | null>(null);
  const [error, setError] = useState("");
  const [unsupported, setUnsupported] = useState(false);
  const [stale, setStale] = useState(false);
  const [detail, setDetail] = useState<Detail | null>(null);
  const [pageAnnouncement, setPageAnnouncement] = useState("");
  const pageRequestRef = useRef<PageKind | null>(null);
  const snapshotRef = useRef<RemoteAutomationMonitorSnapshot | null>(null);
  const retainedPageIdsRef = useRef<RetainedPageIds>({
    active: new Set(),
    recent: new Set(),
    schedules: new Set(),
  });
  const detailOriginRef = useRef<HTMLElement | null>(null);
  const dialogRef = useRef<HTMLDivElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);

  const refresh = useCallback(async () => {
    if (snapshotRef.current) setRefreshing(true);
    else setLoading(true);
    setError("");
    try {
      const next = await remoteClient.loadAutomationMonitor();
      const reconciled = snapshotRef.current
        ? reconcileRefresh(next, snapshotRef.current, retainedPageIdsRef.current)
        : next;
      snapshotRef.current = reconciled;
      setSnapshot(reconciled);
      setUnsupported(false);
      setStale(false);
    } catch (cause) {
      if (cause instanceof RemoteRequestError && cause.status === 404) {
        setUnsupported(true);
      } else if (snapshotRef.current) {
        setStale(true);
        setError(cause instanceof Error ? cause.message : String(cause));
      } else {
        setError(cause instanceof Error ? cause.message : String(cause));
      }
    } finally {
      setLoading(false);
      setRefreshing(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    const resume = () => {
      if (document.visibilityState !== "hidden") void refresh();
    };
    window.addEventListener("focus", resume);
    window.addEventListener("pageshow", resume);
    document.addEventListener("visibilitychange", resume);
    return () => {
      window.removeEventListener("focus", resume);
      window.removeEventListener("pageshow", resume);
      document.removeEventListener("visibilitychange", resume);
    };
  }, [refresh]);

  const loadMore = async (kind: PageKind) => {
    if (!snapshot || pageRequestRef.current) return;
    const offset = kind === "active"
      ? snapshot.active_runs_next_offset
      : kind === "recent"
        ? snapshot.recent_runs_next_offset
        : snapshot.schedules_next_offset;
    if (offset === null) return;
    pageRequestRef.current = kind;
    setLoadingPage(kind);
    setPageAnnouncement("");
    try {
      const page = await remoteClient.loadAutomationMonitor({
        ...(kind === "active" ? { active_offset: offset } : {}),
        ...(kind === "recent" ? { recent_offset: offset } : {}),
        ...(kind === "schedules" ? { schedule_offset: offset } : {}),
      });
      if (kind === "active") {
        for (const run of page.active_runs) retainedPageIdsRef.current.active.add(run.run_id);
      } else if (kind === "recent") {
        for (const run of page.recent_runs) retainedPageIdsRef.current.recent.add(run.run_id);
      } else {
        for (const schedule of page.schedules) retainedPageIdsRef.current.schedules.add(schedule.id);
      }
      setSnapshot((current) => {
        if (!current) return current;
        const activeRuns = uniqueBy(current.active_runs, page.active_runs, (run) => run.run_id);
        const recentRuns = uniqueBy(current.recent_runs, page.recent_runs, (run) => run.run_id);
        const schedules = uniqueBy(current.schedules, page.schedules, (schedule) => schedule.id);
        const added = kind === "active"
          ? activeRuns.length - current.active_runs.length
          : kind === "recent"
            ? recentRuns.length - current.recent_runs.length
            : schedules.length - current.schedules.length;
        const label = kind === "active" ? "active runs" : kind === "recent" ? "outcomes" : "schedules";
        setPageAnnouncement(`${added} ${label} loaded.`);
        const reconciled = {
          ...current,
          generated_at: page.generated_at,
          active_runs: activeRuns,
          active_runs_truncated: kind === "active" ? page.active_runs_truncated : current.active_runs_truncated,
          active_runs_next_offset: kind === "active" ? page.active_runs_next_offset : current.active_runs_next_offset,
          recent_runs: recentRuns,
          recent_runs_truncated: kind === "recent" ? page.recent_runs_truncated : current.recent_runs_truncated,
          recent_runs_next_offset: kind === "recent" ? page.recent_runs_next_offset : current.recent_runs_next_offset,
          schedules,
          schedules_truncated: kind === "schedules" ? page.schedules_truncated : current.schedules_truncated,
          schedules_next_offset: kind === "schedules" ? page.schedules_next_offset : current.schedules_next_offset,
        };
        snapshotRef.current = reconciled;
        return reconciled;
      });
    } catch (cause) {
      setStale(true);
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      pageRequestRef.current = null;
      setLoadingPage(null);
    }
  };

  const closeDetail = useCallback(() => {
    if (window.history.state?.wardian_remote_view === "automation_detail") {
      window.history.back();
      return;
    }
    setDetail(null);
    detailOriginRef.current?.focus();
  }, []);

  const openDetail = (next: Detail, origin: HTMLElement) => {
    detailOriginRef.current = origin;
    setDetail(next);
    const currentHistoryState = window.history.state;
    window.history.pushState(
      {
        ...(currentHistoryState && typeof currentHistoryState === "object" ? currentHistoryState : {}),
        wardian_remote_view: "automation_detail",
      },
      "",
      `${window.location.pathname}${window.location.search}${window.location.hash}`,
    );
  };

  useEffect(() => {
    if (!detail) return;
    closeRef.current?.focus();
    const onPopState = () => {
      setDetail(null);
      detailOriginRef.current?.focus();
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") closeDetail();
      if (event.key !== "Tab" || !dialogRef.current) return;
      const focusable = [...dialogRef.current.querySelectorAll<HTMLElement>("button,[href],[tabindex]:not([tabindex='-1'])")];
      if (focusable.length === 0) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    window.addEventListener("popstate", onPopState);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("popstate", onPopState);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [closeDetail, detail]);

  const attention = useMemo(() => snapshot ? attentionItems(snapshot) : [], [snapshot]);
  const running = useMemo(() => snapshot?.active_runs.filter((run) => run.status === "running") ?? [], [snapshot]);
  const schedules = snapshot?.schedules ?? [];
  const recent = snapshot?.recent_runs ?? [];
  const lastUpdated = snapshot ? formatAutomationTime(snapshot.generated_at).primary : "Not updated";

  return (
    <section className="flex min-h-0 flex-1 flex-col" data-testid="remote-automations-view">
      <header className="shrink-0 border-b border-wardian-border bg-[var(--color-wardian-bg)] px-4 pb-3 pt-[max(0.75rem,env(safe-area-inset-top))]">
        <div className="flex items-center justify-between gap-3">
          <div className="min-w-0">
            <h1 className="truncate text-base font-semibold text-primary">Automations</h1>
            <p className="truncate text-xs text-muted-neutral">Updated {lastUpdated.toLowerCase()}</p>
          </div>
          <button
            type="button"
            aria-label="Refresh automation monitor"
            disabled={loading || refreshing}
            onClick={() => void refresh()}
            className={`inline-flex h-11 w-11 shrink-0 items-center justify-center rounded-md border border-wardian-border text-muted-neutral transition-colors active:bg-wardian-card-bg-muted disabled:opacity-50 ${focusRing}`}
          >
            <RefreshCw className={`h-4 w-4 ${refreshing ? "animate-spin motion-reduce:animate-none" : ""}`} aria-hidden="true" />
          </button>
        </div>
        <div className="mt-3 grid grid-cols-4 gap-1" aria-label="Automation monitor filters">
          {filters.map((item) => (
            <button
              key={item.id}
              type="button"
              aria-pressed={filter === item.id}
              onClick={() => setFilter(item.id)}
              className={`min-h-11 rounded-md border px-1 text-xs font-semibold transition-colors active:bg-wardian-card-bg-muted ${focusRing} ${
                filter === item.id
                  ? "border-[var(--color-wardian-accent)] text-primary"
                  : "border-transparent text-muted-neutral"
              }`}
            >
              {item.label}
            </button>
          ))}
        </div>
      </header>

      <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-3 py-4" data-testid="remote-automation-scroll-region">
        {unsupported ? (
          <StatePanel title="Automation monitoring is unavailable" body="Update the desktop app to use automation monitoring." />
        ) : loading && !snapshot ? (
          <MonitorSkeleton />
        ) : error && !snapshot ? (
          <StatePanel title="Could not load automations" body={error} action="Retry" onAction={() => void refresh()} />
        ) : snapshot ? (
          <div className="mx-auto flex w-full max-w-[720px] flex-col gap-4">
            {stale ? (
              <div role="alert" className="flex items-center justify-between gap-3 rounded-md border border-[var(--color-wardian-warning)] bg-wardian-card-bg px-3 py-2 text-xs text-primary">
                <span>Showing saved results from {lastUpdated}. Refresh failed.</span>
                <button type="button" onClick={() => void refresh()} className={`min-h-11 shrink-0 px-2 font-semibold ${focusRing}`}>Retry</button>
              </div>
            ) : null}

            {filter === "overview" ? (
              <div className="grid grid-cols-3 gap-2" data-testid="automation-summary-shortcuts">
                <SummaryShortcut label="Attention" value={attention.length} tone="warning" onClick={() => setFilter("attention")} />
                <SummaryShortcut label="Running" value={running.length} tone="active" onClick={() => scrollToSection("remote-running-section")} />
                <SummaryShortcut label="Soon" value={schedules.filter((schedule) => !schedule.is_paused).length} tone="accent" onClick={() => setFilter("soon")} />
              </div>
            ) : null}

            {(filter === "overview" || filter === "attention") && attention.length > 0 ? (
              <MonitorSection title="Needs attention" count={attention.length}>
                {attention.map((item) => (
                  item.kind === "run"
                    ? <RunCard key={`attention:${item.value.run_id}`} run={item.value} onOpen={openDetail} />
                    : <ScheduleCard key={`attention:${item.value.id}`} schedule={item.value} onOpen={openDetail} />
                ))}
                {filter === "attention" && snapshot.active_runs_next_offset !== null ? <LoadMore label="Load more active runs" loading={loadingPage === "active"} onClick={() => void loadMore("active")} /> : null}
              </MonitorSection>
            ) : null}
            {filter === "attention" && attention.length === 0 ? <EmptyState>Nothing needs attention.</EmptyState> : null}

            {filter === "overview" && running.length > 0 ? (
              <MonitorSection id="remote-running-section" title="Running now" count={running.length}>
                {running.map((run) => <RunCard key={run.run_id} run={run} onOpen={openDetail} />)}
                {snapshot.active_runs_next_offset !== null ? <LoadMore label="Load more active runs" loading={loadingPage === "active"} onClick={() => void loadMore("active")} /> : null}
              </MonitorSection>
            ) : null}

            {(filter === "overview" || filter === "soon") && schedules.length > 0 ? (
              <MonitorSection title="Up next" count={schedules.length}>
                {schedules.map((schedule) => <ScheduleCard key={schedule.id} schedule={schedule} onOpen={openDetail} />)}
                {snapshot.schedules_next_offset !== null ? <LoadMore label="Load more schedules" loading={loadingPage === "schedules"} onClick={() => void loadMore("schedules")} /> : null}
              </MonitorSection>
            ) : null}
            {filter === "soon" && schedules.length === 0 ? <EmptyState>No schedules are coming up.</EmptyState> : null}

            {(filter === "overview" || filter === "history") && recent.length > 0 ? (
              <MonitorSection title="Recent outcomes" count={recent.length}>
                {(filter === "overview" ? recent.slice(0, 5) : recent).map((run) => <RunCard key={run.run_id} run={run} compact onOpen={openDetail} />)}
                {filter === "overview" && recent.length > 5 ? <LoadMore label="View all history" onClick={() => setFilter("history")} /> : null}
                {filter === "history" && snapshot.recent_runs_next_offset !== null ? <LoadMore label="Show older outcomes" loading={loadingPage === "recent"} onClick={() => void loadMore("recent")} /> : null}
              </MonitorSection>
            ) : null}
            {filter === "history" && recent.length === 0 ? <EmptyState>No recent automation outcomes.</EmptyState> : null}
            {filter === "overview" && attention.length === 0 && running.length === 0 && schedules.length === 0 && recent.length === 0 ? (
              <EmptyState>No automation activity yet.</EmptyState>
            ) : null}
            <p className="sr-only" aria-live="polite">{loadingPage ? `Loading more ${loadingPage}` : pageAnnouncement}</p>
          </div>
        ) : null}
      </div>

      {detail ? <DetailSheet detail={detail} dialogRef={dialogRef} closeRef={closeRef} onClose={closeDetail} /> : null}
    </section>
  );
};

function SummaryShortcut({ label, value, tone, onClick }: { label: string; value: number; tone: "warning" | "active" | "accent"; onClick: () => void }) {
  const color = tone === "warning" ? "var(--color-wardian-warning)" : tone === "active" ? "var(--color-wardian-processing)" : "var(--color-wardian-accent)";
  return <button type="button" onClick={onClick} className={`min-h-16 rounded-md border border-wardian-border bg-wardian-card-bg px-2 py-2 text-left active:bg-wardian-card-bg-muted ${focusRing}`}><span className="block text-lg font-bold" style={{ color }}>{value}</span><span className="block text-xs font-semibold text-primary">{label}</span></button>;
}

function scrollToSection(id: string) {
  const reduceMotion = window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;
  document.getElementById(id)?.scrollIntoView({ behavior: reduceMotion ? "auto" : "smooth", block: "start" });
}

function MonitorSection({ id, title, count, children }: React.PropsWithChildren<{ id?: string; title: string; count: number }>) {
  return <section id={id}><div className="mb-2 flex items-center justify-between"><h2 className="text-sm font-semibold text-primary">{title}</h2><span className="text-xs text-muted-neutral">{count}</span></div><div className="flex flex-col gap-2">{children}</div></section>;
}

function RunCard({ run, compact = false, onOpen }: { run: RemoteAutomationMonitorRun; compact?: boolean; onOpen: (detail: Detail, origin: HTMLElement) => void }) {
  const failed = run.status === "failed";
  const awaiting = run.status === "awaiting_approval";
  const active = run.status === "running";
  const Icon = failed ? XCircle : awaiting ? AlertTriangle : active ? LoaderCircle : CheckCircle2;
  const color = failed ? "var(--color-wardian-error)" : awaiting ? "var(--color-wardian-warning)" : active ? "var(--color-wardian-processing)" : "var(--color-wardian-success)";
  const label = failed ? "Failed" : awaiting ? "Awaiting approval" : active ? "Running" : "Completed";
  const primary = run.failure ?? (active ? `Started ${relativeTime(run.started_at)}` : relativeTime(runTime(run)));
  return <button type="button" onClick={(event) => onOpen({ kind: "run", value: run }, event.currentTarget)} className={`relative flex w-full items-center gap-3 overflow-hidden rounded-md border border-wardian-border bg-wardian-card-bg px-3 text-left active:bg-wardian-card-bg-muted ${focusRing} ${compact ? "min-h-[72px] py-2" : "min-h-[88px] py-3"}`}><span className="absolute inset-y-0 left-0 w-1" style={{ backgroundColor: color }} /><Icon className={`ml-1 h-5 w-5 shrink-0 ${active ? "animate-spin motion-reduce:animate-none" : ""}`} style={{ color }} aria-hidden="true" /><span className="min-w-0 flex-1"><span className="block truncate text-sm font-semibold text-primary">{run.automation_name}</span><span className="mt-0.5 block truncate text-xs font-semibold" style={{ color }}>{label}</span><span className="mt-1 block truncate text-xs text-muted-neutral">{primary}</span></span><ChevronRight className="h-4 w-4 shrink-0 text-muted-neutral" aria-hidden="true" /></button>;
}

function ScheduleCard({ schedule, onOpen }: { schedule: RemoteAutomationMonitorSchedule; onOpen: (detail: Detail, origin: HTMLElement) => void }) {
  const paused = schedule.is_paused;
  const failed = schedule.last_run_status === "failed";
  const Icon = paused ? PauseCircle : failed ? AlertTriangle : Clock3;
  const color = failed ? "var(--color-wardian-error)" : paused ? "var(--color-wardian-warning)" : "var(--color-wardian-accent)";
  const exact = paused ? "Paused" : formatAutomationTime(schedule.next_run_epoch_ms, { emptyLabel: "Not scheduled" }).primary;
  return <button type="button" onClick={(event) => onOpen({ kind: "schedule", value: schedule }, event.currentTarget)} className={`relative flex min-h-[88px] w-full items-center gap-3 overflow-hidden rounded-md border border-wardian-border bg-wardian-card-bg px-3 py-3 text-left active:bg-wardian-card-bg-muted ${focusRing}`}><span className="absolute inset-y-0 left-0 w-1" style={{ backgroundColor: color }} /><Icon className="ml-1 h-5 w-5 shrink-0" style={{ color }} aria-hidden="true" /><span className="min-w-0 flex-1"><span className="block truncate text-sm font-semibold text-primary">{schedule.automation_name}</span><span className="mt-0.5 block truncate text-xs font-semibold" style={{ color }}>{paused ? "Paused" : relativeTime(schedule.next_run_epoch_ms)}</span><span className="mt-1 block truncate text-xs text-muted-neutral">{exact} · {scheduleSummaryLabel(schedule.schedule)}</span></span><ChevronRight className="h-4 w-4 shrink-0 text-muted-neutral" aria-hidden="true" /></button>;
}

function LoadMore({ label, loading = false, onClick }: { label: string; loading?: boolean; onClick: () => void }) {
  return <button type="button" disabled={loading} onClick={onClick} className={`min-h-11 w-full rounded-md border border-wardian-border px-3 text-xs font-semibold text-primary active:bg-wardian-card-bg-muted disabled:opacity-50 ${focusRing}`}>{loading ? "Loading…" : label}</button>;
}

function EmptyState({ children }: React.PropsWithChildren) {
  return <div className="rounded-md border border-dashed border-wardian-border px-3 py-6 text-center text-sm text-muted-neutral">{children}</div>;
}

function StatePanel({ title, body, action, onAction }: { title: string; body: string; action?: string; onAction?: () => void }) {
  return <div className="mx-auto max-w-md rounded-md border border-wardian-border bg-wardian-card-bg px-4 py-6 text-center"><h2 className="text-sm font-semibold text-primary">{title}</h2><p className="mt-2 break-words text-xs leading-5 text-muted-neutral">{body}</p>{action && onAction ? <button type="button" onClick={onAction} className={`mt-4 min-h-11 rounded-md border border-wardian-border px-4 text-sm font-semibold text-primary ${focusRing}`}>{action}</button> : null}</div>;
}

function MonitorSkeleton() {
  return <div aria-label="Loading automations" className="mx-auto flex max-w-[720px] animate-pulse flex-col gap-3 motion-reduce:animate-none">{[0, 1, 2, 3].map((value) => <div key={value} className="h-[88px] rounded-md border border-wardian-border bg-wardian-card-bg-muted" />)}</div>;
}

function DetailSheet({ detail, dialogRef, closeRef, onClose }: { detail: Detail; dialogRef: React.RefObject<HTMLDivElement | null>; closeRef: React.RefObject<HTMLButtonElement | null>; onClose: () => void }) {
  const isRun = detail.kind === "run";
  const name = detail.value.automation_name;
  return <div className="fixed inset-0 z-50 flex items-end bg-[color-mix(in_srgb,var(--color-wardian-bg),transparent_35%)]" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}><div ref={dialogRef} role="dialog" aria-modal="true" aria-labelledby="automation-detail-title" className="max-h-[82dvh] w-full overflow-y-auto rounded-t-xl border border-wardian-border bg-[var(--color-wardian-bg)] px-4 pb-[max(1rem,env(safe-area-inset-bottom))] pt-3 shadow-2xl"><div className="mx-auto mb-3 h-1 w-10 rounded-full bg-[var(--color-wardian-border)]" aria-hidden="true" /><div className="flex items-start justify-between gap-3"><div className="min-w-0"><h2 id="automation-detail-title" className="truncate text-base font-semibold text-primary">{name}</h2><p className="mt-1 text-xs text-muted-neutral">{isRun ? "Automation run" : "Schedule"}</p></div><button ref={closeRef} type="button" aria-label="Close automation details" onClick={onClose} className={`inline-flex h-11 w-11 shrink-0 items-center justify-center rounded-md border border-wardian-border text-muted-neutral ${focusRing}`}><X className="h-4 w-4" aria-hidden="true" /></button></div>{isRun ? <RunDetails run={detail.value} /> : <ScheduleDetails schedule={detail.value} />}</div></div>;
}

function RunDetails({ run }: { run: RemoteAutomationMonitorRun }) {
  return <dl className="mt-5 grid gap-4 text-sm"><DetailRow label="Status" value={run.status.replace(/_/g, " ")} /><DetailRow label="Started" value={formatAutomationTime(run.started_at).primary} /><DetailRow label="Last update" value={formatAutomationTime(runTime(run)).primary} /><DetailRow label="Nodes observed" value={String(run.node_count)} />{run.failure ? <DetailRow label="Failure" value={run.failure} alert /> : null}</dl>;
}

function ScheduleDetails({ schedule }: { schedule: RemoteAutomationMonitorSchedule }) {
  return <dl className="mt-5 grid gap-4 text-sm"><DetailRow label="Next run" value={schedule.is_paused ? "Paused" : formatAutomationTime(schedule.next_run_epoch_ms, { emptyLabel: "Not scheduled" }).primary} /><DetailRow label="Schedule" value={scheduleSummaryLabel(schedule.schedule)} /><DetailRow label="Last outcome" value={schedule.last_run_status?.replace(/_/g, " ") ?? "Never run"} />{schedule.target_labels.length > 0 ? <DetailRow label="Targets" value={schedule.target_labels.join(", ")} /> : null}{schedule.last_run_error ? <DetailRow label="Last error" value={schedule.last_run_error} alert /> : null}</dl>;
}

function DetailRow({ label, value, alert = false }: { label: string; value: string; alert?: boolean }) {
  return <div><dt className="text-xs font-semibold text-muted-neutral">{label}</dt><dd role={alert ? "alert" : undefined} className={`mt-1 break-words capitalize ${alert ? "text-[var(--color-wardian-error)]" : "text-primary"}`}>{value}</dd></div>;
}
