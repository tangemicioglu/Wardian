import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Check } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import type {
  AgentConfig,
  ChangeReviewBaseline,
  ChangeReviewFileEntry,
  ChangeReviewLoadResponse,
  ChangeReviewPrefs,
  ChangeReviewReviewedPath,
  ChangeReviewSummary,
  ChangeReviewWatermark,
  FilesComparisonBaseline,
  FilesSurfaceStateV2,
} from "../../types";
import { useAppShellWorkbenchNavigation } from "../../layout/AppShell";
import type { WorkbenchNavigationService } from "../workbench/navigationService";
import { fileResourceKey } from "../files/fileResourceKey";
import { normalizeExplorerPathForCompare } from "../explorer/pathUtils";

interface ExplorerChangedEvent {
  root_path: string;
  changed_paths: string[];
}

type ChangesPanelProps = {
  visible: boolean;
  agents: readonly AgentConfig[];
  selected_agent_ids: ReadonlySet<string>;
  turn_revision: number;
  navigation?: WorkbenchNavigationService | null;
};

const BASELINE_OPTIONS: readonly { value: ChangeReviewBaseline; label: string }[] = [
  { value: "last_effective_turn", label: "Last turn" },
  { value: "conversation_start", label: "This conversation" },
  { value: "branch_point", label: "This branch" },
  { value: "head", label: "Last commit" },
  { value: "unreviewed", label: "I last looked" },
];

const DEFAULT_CHANGE_REVIEW_BASELINE: ChangeReviewBaseline = "last_effective_turn";

const CHANGE_KIND_PRESENTATION: Record<ChangeReviewFileEntry["change_kind"], {
  label: string;
  markerClass: string;
  textClass: string;
}> = {
  added: {
    label: "Added",
    markerClass: "bg-[var(--color-wardian-success)]",
    textClass: "text-[var(--color-wardian-success)]",
  },
  modified: {
    label: "Modified",
    markerClass: "bg-[var(--color-wardian-warning)]",
    textClass: "text-[var(--color-wardian-warning)]",
  },
  deleted: {
    label: "Deleted",
    markerClass: "bg-[var(--color-wardian-error)]",
    textClass: "text-[var(--color-wardian-error)]",
  },
  renamed: {
    label: "Renamed",
    markerClass: "bg-[var(--color-wardian-processing)]",
    textClass: "text-[var(--color-wardian-processing)]",
  },
  untracked: {
    label: "Untracked",
    markerClass: "bg-[var(--color-wardian-success)]",
    textClass: "text-[var(--color-wardian-success)]",
  },
};

function splitDisplayPath(path: string): { directory: string; filename: string } {
  const normalized = path.replace(/\\/g, "/");
  const separator = normalized.lastIndexOf("/");
  return separator === -1
    ? { directory: "", filename: normalized }
    : { directory: normalized.slice(0, separator), filename: normalized.slice(separator + 1) };
}

function changeEvidenceLabel(entry: ChangeReviewFileEntry): string {
  return entry.evidence === "attributed"
    ? "attributed to an agent edit"
    : "detected from workspace changes";
}

function isChangeReviewBaseline(value: unknown): value is ChangeReviewBaseline {
  return BASELINE_OPTIONS.some((option) => option.value === value);
}

function baselineFromPrefs(prefs: unknown): ChangeReviewBaseline {
  if (typeof prefs !== "object" || prefs === null) return DEFAULT_CHANGE_REVIEW_BASELINE;
  const candidate = prefs as { schema?: unknown; baseline?: unknown };
  return candidate.schema === 1 && isChangeReviewBaseline(candidate.baseline)
    ? candidate.baseline
    : DEFAULT_CHANGE_REVIEW_BASELINE;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function pathForWorkspace(workspace: string, path: string): string {
  if (/^[A-Za-z]:[\\/]/.test(path) || path.startsWith("/") || path.startsWith("\\\\")) {
    return path;
  }
  return `${workspace.replace(/[\\/]+$/g, "")}/${path.replace(/^[\\/]+/g, "")}`;
}

function sameWorkspacePath(left: string, right: string): boolean {
  return normalizeExplorerPathForCompare(left) === normalizeExplorerPathForCompare(right);
}

/**
 * Builds the comparison baseline for a change-review entry.
 *
 * `old_path` is used when present so a renamed file is compared against the name
 * it had at the baseline. Added and untracked files are marked `absent` rather
 * than pointed at a revision that does not contain them.
 */
function baselineForFile(
  entry: ChangeReviewFileEntry,
  baselineRef: string | null,
  workspace: string,
  label: string,
): FilesComparisonBaseline {
  return {
    kind: "git_revision",
    revision: baselineRef ?? "HEAD",
    cwd: workspace,
    path: entry.old_path ?? entry.path,
    label,
    absent: entry.change_kind === "added" || entry.change_kind === "untracked",
  };
}

/** Opens a change-review diff in the workbench with its comparison already open. */
function changeSurfaceState(baseline: FilesComparisonBaseline): FilesSurfaceStateV2 {
  return {
    resource_kind: "file",
    transient_preview: false,
    presentation: "editor",
    comparison_open: true,
    comparison_layout_preference: "auto",
    comparison_baseline: baseline,
    review_drawer_open: false,
    selected_version_id: null,
    optional_checkpoint_id: null,
  };
}

export function ChangesPanel({
  visible,
  agents,
  selected_agent_ids,
  turn_revision,
  navigation,
}: ChangesPanelProps) {
  const appShellNavigation = useAppShellWorkbenchNavigation();
  const workbenchNavigation = navigation === undefined ? appShellNavigation : navigation;
  const selectedAgentId = selected_agent_ids.size === 1
    ? [...selected_agent_ids][0]
    : null;
  const selectedAgent = agents.find((agent) => agent.session_id === selectedAgentId) ?? null;
  const [workspace, setWorkspace] = useState<string | null>(null);
  const [baseline, setBaseline] = useState<ChangeReviewBaseline>(DEFAULT_CHANGE_REVIEW_BASELINE);
  const [prefsLoaded, setPrefsLoaded] = useState(false);
  const [summary, setSummary] = useState<ChangeReviewSummary | null>(null);
  const [gitAvailable, setGitAvailable] = useState(true);
  const [headRef, setHeadRef] = useState<string | null>(null);
  const [openedPath, setOpenedPath] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [openError, setOpenError] = useState<string | null>(null);
  const requestGeneration = useRef(0);

  useEffect(() => {
    let active = true;
    void invoke<ChangeReviewPrefs>("load_change_review_prefs")
      .then((prefs) => {
        if (!active) return;
        setBaseline(baselineFromPrefs(prefs));
        setPrefsLoaded(true);
      })
      .catch(() => {
        if (!active) return;
        setBaseline(DEFAULT_CHANGE_REVIEW_BASELINE);
        setPrefsLoaded(true);
      });
    return () => { active = false; };
  }, []);

  useEffect(() => {
    let active = true;
    setWorkspace(null);
    if (!selectedAgentId || !selectedAgent) return () => { active = false; };
    void invoke<string>("get_explorer_root", { sessionId: selectedAgentId })
      .then((root) => {
        if (active) setWorkspace(root.trim() || selectedAgent.folder.trim() || null);
      })
      .catch(() => {
        if (active) setWorkspace(selectedAgent.folder.trim() || null);
      });
    return () => { active = false; };
  }, [selectedAgent, selectedAgentId]);

  const recompute = useCallback(async () => {
    if (!prefsLoaded || !workspace || !selectedAgentId || !visible) return;
    const generation = ++requestGeneration.current;
    setLoading(true);
    setError(null);
    try {
      const response = await invoke<ChangeReviewLoadResponse>("load_change_review", {
        request: {
          cwd: workspace,
          baseline,
          agent_id: selectedAgentId,
        },
      });
      if (generation !== requestGeneration.current) return;
      setSummary(response.summary);
      setGitAvailable(response.git_available);
      setHeadRef(response.head_ref);
    } catch (reason) {
      if (generation === requestGeneration.current) setError(errorMessage(reason));
    } finally {
      if (generation === requestGeneration.current) setLoading(false);
    }
  }, [baseline, prefsLoaded, selectedAgentId, visible, workspace]);

  useEffect(() => {
    void recompute();
  }, [recompute, turn_revision]);

  useEffect(() => {
    if (!workspace || !visible) return;
    let active = true;
    const unlistenPromise = listen<string>("git-changed", (event) => {
      if (active && sameWorkspacePath(event.payload, workspace)) void recompute();
    });
    return () => {
      active = false;
      void unlistenPromise.then((unlisten) => unlisten());
    };
  }, [recompute, visible, workspace]);

  useEffect(() => {
    if (!workspace || !visible) return;

    let disposed = false;
    let watchActive = false;
    const unlistenPromise = listen<ExplorerChangedEvent>("explorer-changed", (event) => {
      if (
        normalizeExplorerPathForCompare(event.payload.root_path) !==
        normalizeExplorerPathForCompare(workspace)
      ) {
        return;
      }
      void recompute();
    });

    void unlistenPromise
      .then(async () => {
        if (disposed) return;
        await invoke("explorer_watch", { rootPath: workspace });
        watchActive = true;
        if (disposed) {
          watchActive = false;
          void invoke("explorer_unwatch", { rootPath: workspace }).catch(() => undefined);
        }
      })
      .catch((reason: unknown) => {
        if (!disposed) console.error("Failed to watch Changes workspace:", reason);
      });

    return () => {
      disposed = true;
      if (watchActive) {
        watchActive = false;
        void invoke("explorer_unwatch", { rootPath: workspace }).catch(() => undefined);
      }
      void unlistenPromise.then((unlisten) => unlisten()).catch(() => undefined);
    };
  }, [recompute, visible, workspace]);

  const reviewPathOnExpand = useCallback(async (path: string) => {
    if (!workspace || !selectedAgentId || !summary) return;
    const entry = summary.files.find((file) => file.path === path);
    if (!entry) return;

    const existing = await invoke<ChangeReviewWatermark | null>("load_change_review_watermark", {
      agent_id: selectedAgentId,
      workspace,
    }).catch(() => null);
    const reviewed_paths: ChangeReviewReviewedPath[] = [...(existing?.reviewed_paths ?? [])];
    const alreadyReviewed = reviewed_paths.some((reviewedPath) => (
      reviewedPath.path === entry.path
      && reviewedPath.change_kind === entry.change_kind
      && reviewedPath.insertions === entry.insertions
      && reviewedPath.deletions === entry.deletions
    ));
    if (!alreadyReviewed) {
      reviewed_paths.push({
        path: entry.path,
        change_kind: entry.change_kind,
        insertions: entry.insertions,
        deletions: entry.deletions,
      });
    }

    await invoke("save_change_review_watermark", {
      watermark: {
        schema: 1,
        agent_id: selectedAgentId,
        workspace,
        reviewed_turn_index: summary.to_turn_index ?? existing?.reviewed_turn_index ?? 0,
        reviewed_at: new Date().toISOString(),
        reviewed_head: headRef,
        reviewed_paths,
      },
    }).catch(() => undefined);
  }, [headRef, selectedAgentId, summary, workspace]);

  const baselineLabel = useMemo(
    () => BASELINE_OPTIONS.find((option) => option.value === baseline)?.label ?? "Baseline",
    [baseline],
  );

  const openFile = useCallback((entry: ChangeReviewFileEntry) => {
    if (!workspace) return;
    if (!workbenchNavigation) {
      setOpenError("Workbench navigation is unavailable.");
      return;
    }
    if (entry.binary) {
      setOpenError("Binary content is listed but not rendered.");
      return;
    }
    try {
      const surfaceId = workbenchNavigation.open({
        surface_type: "files",
        resource_key: fileResourceKey(pathForWorkspace(workspace, entry.path)),
        state: changeSurfaceState(
          baselineForFile(entry, summary?.baseline_ref ?? null, workspace, baselineLabel),
        ),
      });
      workbenchNavigation.pin_transient(surfaceId);
      setOpenError(null);
      setOpenedPath(entry.path);
      // Opening the diff is the act of reviewing, as expanding was before it.
      void reviewPathOnExpand(entry.path);
    } catch (error) {
      setOpenError(`Unable to open the comparison: ${errorMessage(error)}`);
    }
  }, [baselineLabel, reviewPathOnExpand, summary?.baseline_ref, workbenchNavigation, workspace]);

  const handleBaselineChange = useCallback((nextBaseline: ChangeReviewBaseline) => {
    if (nextBaseline === baseline) return;
    setOpenedPath(null);
    setBaseline(nextBaseline);
    const prefs: ChangeReviewPrefs = { schema: 1, baseline: nextBaseline };
    void invoke("save_change_review_prefs", { prefs }).catch(() => undefined);
  }, [baseline]);

  return (
    <section
      aria-hidden={!visible}
      className="flex h-full min-h-0 min-w-0 flex-col text-[var(--color-wardian-text)]"
      data-testid="changes-panel"
    >
      <header className="flex shrink-0 flex-wrap items-center gap-2 border-b border-[var(--color-wardian-border)] pb-3">
        <div className="mr-auto min-w-0">
          <h2 className="text-sm font-bold text-primary tracking-tight">Changes</h2>
        </div>
        <label className="flex w-full items-center gap-2 text-[11px]">
           <span className="text-[var(--color-wardian-text-muted)]">Since</span>
          <select
            aria-label="Change review baseline"
            className="min-w-0 flex-1 rounded border border-[var(--color-wardian-border)] bg-[var(--color-wardian-input-bg)] px-1.5 py-1 text-[11px]"
            value={baseline}
            onChange={(event) => {
              const nextBaseline = event.currentTarget.value;
              if (isChangeReviewBaseline(nextBaseline)) handleBaselineChange(nextBaseline);
            }}
          >
            {BASELINE_OPTIONS.map((option) => <option key={option.value} value={option.value}>{option.label}</option>)}
          </select>
        </label>
      </header>
      {!selectedAgent ? (
        <div className="mt-3 rounded border border-[var(--color-wardian-border)] p-3 text-[11px] text-[var(--color-wardian-text-muted)]" role="status">
          Select one agent to inspect its workspace changes.
        </div>
      ) : !workspace ? (
        <div className="mt-3 rounded border border-[var(--color-wardian-border)] p-3 text-[11px] text-[var(--color-wardian-text-muted)]" role="status">
          Resolving the selected agent workspace…
        </div>
      ) : loading && !summary ? (
        <div className="mt-3 p-3 text-[11px] text-[var(--color-wardian-text-muted)]" role="status">Computing changes…</div>
      ) : (
        <div className="min-h-0 flex-1 overflow-auto pt-3">
          {error ? (
            <div className="mb-3 rounded border border-[var(--color-wardian-border)] p-3 text-[11px]" role="alert">
              Unable to refresh change attribution; showing the last successful change set.
            </div>
          ) : null}
          {!gitAvailable ? (
            <div className="mb-3 rounded border border-[var(--color-wardian-border)] p-3 text-[11px] text-[var(--color-wardian-text-muted)]" role="status">
              This workspace is not a git repository. Turn-record file claims are shown without diff content.
            </div>
          ) : null}
          {openError ? (
            <div className="mb-3 rounded border border-[var(--color-wardian-border)] p-3 text-[11px]" role="alert">
              {openError}
            </div>
          ) : null}
          {summary?.diverged ? (
            <div
              className="mb-3 rounded border border-[var(--color-wardian-border)] p-3 text-[11px]"
              role="status"
            >
              <p className="mb-2 text-[var(--color-wardian-text-muted)]">
                This baseline has drifted{" "}
                {summary.turns_since_baseline !== null
                  ? `${summary.turns_since_baseline} turns and `
                  : ""}
                {summary.paths_since_baseline} files. Holding it keeps every
                superseded version of those files on disk.
              </p>
              {/* Re-anchoring is offered, never applied silently: the pinned
                  baseline is the operator's explicit choice of comparison, and
                  moving it without consent answers a different question. */}
              <button
                type="button"
                className="rounded border border-[var(--color-wardian-border)] px-2 py-1 hover:bg-wardian-card-bg-muted"
                onClick={() => handleBaselineChange("last_effective_turn")}
              >
                Compare from the last turn instead
              </button>
            </div>
          ) : null}
          <div className="mb-2 flex flex-wrap items-center gap-2 text-[11px] text-[var(--color-wardian-text-muted)]">
            <span>{summary?.files.length ?? 0} {summary?.files.length === 1 ? "file" : "files"}</span>
            {summary?.from_turn_index !== null && summary?.from_turn_index !== undefined ? <span>from turn {summary.from_turn_index}</span> : null}
            {summary?.to_turn_index !== null && summary?.to_turn_index !== undefined ? <span>through turn {summary.to_turn_index}</span> : null}
          </div>
          <div className="overflow-hidden rounded border border-[var(--color-wardian-border)]">
            {summary?.files.map((entry) => {
              const opened = openedPath === entry.path;
              const { directory, filename } = splitDisplayPath(entry.path);
              const presentation = CHANGE_KIND_PRESENTATION[entry.change_kind];
              return (
                <button
                  type="button"
                  key={entry.path}
                  aria-current={opened ? "true" : undefined}
                  title={entry.path}
                  className={`flex min-h-8 w-full items-center gap-2 border-b border-[var(--color-wardian-border)] px-2 py-1.5 text-left last:border-b-0 hover:bg-wardian-card-bg-muted${
                    opened ? " bg-wardian-card-bg-muted" : ""
                  }`}
                  onClick={() => openFile(entry)}
                >
                  <span
                    aria-label={`${presentation.label} change, ${changeEvidenceLabel(entry)}`}
                    className={`h-1.5 w-1.5 shrink-0 rounded-full ${presentation.markerClass}`}
                  />
                  <span className="min-w-0 flex-1 truncate text-[11px] leading-5">
                    <span className={`font-medium ${presentation.textClass}`}>{filename}</span>
                    {directory ? <span className="ml-1.5 text-[var(--color-wardian-text-muted)]">{directory}</span> : null}
                  </span>
                  <span className="sr-only">{entry.path}</span>
                  <span className="flex shrink-0 items-center gap-1 font-mono text-[10px] tabular-nums">
                    {entry.binary ? (
                      <span className="text-[var(--color-wardian-text-muted)]">Binary</span>
                    ) : (
                      <>
                        {entry.insertions !== null ? <span className="text-[var(--color-wardian-success)]">+{entry.insertions}</span> : null}
                        {entry.deletions !== null ? <span className="text-[var(--color-wardian-error)]">-{entry.deletions}</span> : null}
                        {entry.insertions === null && entry.deletions === null ? <span className="text-[var(--color-wardian-text-muted)]">—</span> : null}
                      </>
                    )}
                    {entry.reviewed ? (
                      <span aria-label="Reviewed" title="Reviewed" className="text-[var(--color-wardian-success)]">
                        <Check className="h-3 w-3" strokeWidth={2.5} aria-hidden="true" />
                      </span>
                    ) : null}
                  </span>
                </button>
              );
            })}
            {summary?.files.length === 0 ? <div className="p-3 text-[11px] text-[var(--color-wardian-text-muted)]">No changes in this baseline.</div> : null}
          </div>
        </div>
      )}
    </section>
  );
}
