import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ChevronDown, ChevronRight } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import type {
  AgentConfig,
  ChangeReviewBaseline,
  ChangeReviewFileEntry,
  ChangeReviewLoadResponse,
  ChangeReviewPrefs,
  ChangeReviewSummary,
  FilesComparisonBaseline,
} from "../../types";
import { FileComparisonLens } from "../files/FileComparisonLens";
import { FileEditorControllerRegistry } from "../files/fileEditorController";
import { fileResourceClient, type FileResourceClient } from "../files/fileResourceClient";
import { useFileResource } from "../files/useFileResource";

type ChangesPanelProps = {
  visible: boolean;
  agents: readonly AgentConfig[];
  selected_agent_ids: ReadonlySet<string>;
  turn_revision: number;
  editor_registry: FileEditorControllerRegistry;
  client?: FileResourceClient;
};

const BASELINE_OPTIONS: readonly { value: ChangeReviewBaseline; label: string }[] = [
  { value: "last_effective_turn", label: "Last effective turn" },
  { value: "conversation_start", label: "Conversation start" },
  { value: "branch_point", label: "Branch point" },
  { value: "head", label: "HEAD" },
  { value: "unreviewed", label: "Unreviewed" },
];

const DEFAULT_CHANGE_REVIEW_BASELINE: ChangeReviewBaseline = "last_effective_turn";

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

function extensionLanguage(path: string): string {
  const extension = path.split(/[\\/]/).pop()?.split(".").pop()?.toLowerCase();
  return extension && extension.length > 0 ? extension : "plaintext";
}

function sameWorkspacePath(left: string, right: string): boolean {
  const normalize = (value: string) => value.replace(/\\/g, "/").replace(/\/+$/g, "").toLocaleLowerCase();
  return normalize(left) === normalize(right);
}

function baselineForFile(entry: ChangeReviewFileEntry, baselineRef: string | null): FilesComparisonBaseline {
  return {
    kind: "prompt_checkpoint",
    checkpoint_id: `change-review:${baselineRef ?? "working-tree"}:${entry.path}`,
  };
}

function ChangeFileComparison({
  panel_key,
  workspace,
  agent_id,
  entry,
  baseline_ref,
  visible,
  editor_registry,
  client = fileResourceClient,
  on_close,
}: {
  panel_key: string;
  workspace: string;
  agent_id: string;
  entry: ChangeReviewFileEntry;
  baseline_ref: string | null;
  visible: boolean;
  editor_registry: FileEditorControllerRegistry;
  client?: FileResourceClient;
  on_close: () => void;
}) {
  const path = pathForWorkspace(workspace, entry.path);
  const resourceRequest = useMemo(() => ({
    path,
    agent_id: agent_id || null,
    user_file_capability_id: null,
  }), [agent_id, path]);
  const resource = useFileResource(resourceRequest, client);
  const snapshot = resource.snapshot;
  const controller = useMemo(
    () => snapshot ? editor_registry.forResource(snapshot.resource_id) : null,
    [editor_registry, snapshot],
  );
  const [baselineText, setBaselineText] = useState<string | null>(null);
  const [baselineError, setBaselineError] = useState<string | null>(null);
  const comparisonSurfaceId = `${panel_key}:comparison:${entry.path}`;

  useEffect(() => {
    if (!controller || !snapshot) return;
    const membership = controller.attachPresentation(comparisonSurfaceId, {});
    let active = true;
    void editor_registry.synchronizeAuthoritative(
      snapshot,
      () => client.readText(snapshot),
      comparisonSurfaceId,
    ).catch((error: unknown) => {
      if (active) setBaselineError(`Unable to load current file: ${errorMessage(error)}`);
    });
    return () => {
      active = false;
      membership.detach();
      editor_registry.releaseAfterPostcommit(
        snapshot.resource_id,
        controller.getSnapshot().presentation_generation,
      );
    };
  }, [client, comparisonSurfaceId, controller, editor_registry, snapshot]);

  useEffect(() => {
    let active = true;
    setBaselineText(null);
    setBaselineError(null);
    const contentRevision = baseline_ref ?? "HEAD";
    if (entry.change_kind === "added" || entry.change_kind === "untracked") {
      setBaselineText("");
      return () => { active = false; };
    }
    void invoke<string>("git_show_file_revision", {
      cwd: workspace,
      path: entry.old_path ?? entry.path,
      revision: contentRevision,
    }).then((text) => {
      if (active) setBaselineText(text);
    }).catch((error: unknown) => {
      if (active) setBaselineError(`Unable to load baseline: ${errorMessage(error)}`);
    });
    return () => { active = false; };
  }, [baseline_ref, entry.change_kind, entry.old_path, entry.path, workspace]);

  if (entry.binary) {
    return <div className="px-3 py-2 text-sm text-[var(--color-wardian-text-muted)]">Binary content is listed but not rendered.</div>;
  }
  if (resource.status === "loading" || !controller || !snapshot) {
    return <div className="px-3 py-2 text-sm text-[var(--color-wardian-text-muted)]" role="status">Loading file comparison…</div>;
  }
  if (resource.status === "error" || resource.error || baselineError) {
    return <div className="px-3 py-2 text-sm text-[var(--color-wardian-text-muted)]" role="status">{baselineError ?? resource.error?.message ?? "File content is unavailable."}</div>;
  }

  return baselineText === null ? (
    <div className="px-3 py-2 text-sm text-[var(--color-wardian-text-muted)]" role="status">Loading baseline…</div>
  ) : (
    <FileComparisonLens
      controller={controller}
      surface_id={comparisonSurfaceId}
      baseline={baselineForFile(entry, baseline_ref)}
      baseline_text={baselineText}
      layout_preference="auto"
      language={extensionLanguage(entry.path)}
      lifecycle={{ visible }}
      on_close={on_close}
      on_layout_preference_change={() => undefined}
      on_reload_from_disk={async () => undefined}
      on_keep_working_buffer={() => undefined}
      on_merge={async () => undefined}
    />
  );
}

export function ChangesPanel({
  visible,
  agents,
  selected_agent_ids,
  turn_revision,
  editor_registry,
  client = fileResourceClient,
}: ChangesPanelProps) {
  const selectedAgentId = selected_agent_ids.size === 1
    ? [...selected_agent_ids][0]
    : null;
  const selectedAgent = agents.find((agent) => agent.session_id === selectedAgentId) ?? null;
  const [workspace, setWorkspace] = useState<string | null>(null);
  const [baseline, setBaseline] = useState<ChangeReviewBaseline>(DEFAULT_CHANGE_REVIEW_BASELINE);
  const [prefsLoaded, setPrefsLoaded] = useState(false);
  const [summary, setSummary] = useState<ChangeReviewSummary | null>(null);
  const [gitAvailable, setGitAvailable] = useState(true);
  const [expandedPath, setExpandedPath] = useState<string | null>(null);
  const [expandedPaths, setExpandedPaths] = useState<ReadonlySet<string>>(new Set());
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [skippedTurnRecords, setSkippedTurnRecords] = useState(0);
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
      setSkippedTurnRecords(response.skipped_turn_records);
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

  const toggleFile = (path: string) => {
    setExpandedPath((current) => current === path ? null : path);
    setExpandedPaths((current) => {
      if (current.has(path)) return current;
      const next = new Set(current);
      next.add(path);
      return next;
    });
  };

  const handleBaselineChange = useCallback((nextBaseline: ChangeReviewBaseline) => {
    if (nextBaseline === baseline) return;
    setExpandedPath(null);
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
          <span className="text-[var(--color-wardian-text-muted)]">Baseline</span>
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
          {skippedTurnRecords > 0 ? (
            <div className="mb-3 rounded border border-[var(--color-wardian-border)] p-3 text-[11px] text-[var(--color-wardian-text-muted)]" role="status">
              Attribution degraded: {skippedTurnRecords} turn {skippedTurnRecords === 1 ? "record was" : "records were"} not readable.
            </div>
          ) : null}
          {!gitAvailable ? (
            <div className="mb-3 rounded border border-[var(--color-wardian-border)] p-3 text-[11px] text-[var(--color-wardian-text-muted)]" role="status">
              This workspace is not a git repository. Turn-record file claims are shown without diff content.
            </div>
          ) : null}
          <div className="mb-2 flex flex-wrap items-center gap-2 text-[11px] text-[var(--color-wardian-text-muted)]">
            <span>{summary?.files.length ?? 0} {summary?.files.length === 1 ? "file" : "files"}</span>
            {summary?.from_turn_index !== null && summary?.from_turn_index !== undefined ? <span>from turn {summary.from_turn_index}</span> : null}
            {summary?.to_turn_index !== null && summary?.to_turn_index !== undefined ? <span>through turn {summary.to_turn_index}</span> : null}
          </div>
          <div className="overflow-hidden rounded border border-[var(--color-wardian-border)]">
            {summary?.files.map((entry) => {
              const expanded = expandedPath === entry.path;
              const hasExpanded = expandedPaths.has(entry.path);
              return (
                <div className="border-b border-[var(--color-wardian-border)] last:border-b-0" key={entry.path}>
                  <button
                    type="button"
                    className="flex w-full items-center gap-1.5 px-2 py-2 text-left hover:bg-wardian-card-bg-muted"
                    aria-expanded={expanded}
                    onClick={() => toggleFile(entry.path)}
                  >
                    {expanded ? <ChevronDown size={14} aria-hidden="true" /> : <ChevronRight size={14} aria-hidden="true" />}
                    <span className="min-w-0 flex-1 truncate font-mono text-[11px]">{entry.path}</span>
                    <span className="text-[10px] text-[var(--color-wardian-text-muted)]">{entry.evidence}</span>
                    <span className="text-[10px] text-[var(--color-wardian-text-muted)]">{entry.insertions ?? "—"}/{entry.deletions ?? "—"}</span>
                  </button>
                  {hasExpanded ? (
                    <div
                      aria-hidden={!expanded}
                      className={expanded
                        ? "relative h-80 min-h-0 border-t border-[var(--color-wardian-border)]"
                        : "hidden"}
                    >
                      <ChangeFileComparison
                        panel_key="changes"
                        workspace={workspace}
                        agent_id={selectedAgentId ?? ""}
                        entry={entry}
                        baseline_ref={summary?.baseline_ref ?? null}
                        editor_registry={editor_registry}
                        client={client}
                        on_close={() => setExpandedPath(null)}
                        visible={expanded && visible}
                      />
                    </div>
                  ) : null}
                </div>
              );
            })}
            {summary?.files.length === 0 ? <div className="p-3 text-[11px] text-[var(--color-wardian-text-muted)]">No changes in this baseline.</div> : null}
          </div>
        </div>
      )}
    </section>
  );
}
