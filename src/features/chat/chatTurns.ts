import type { AgentChatEvent } from "../../types";
import { changedPathsFromEvents, type PresentedChatRow } from "../grid/workLogPresentation";
import { toolPatchText } from "./chatPresentation";
import { structuredEditFromEvent } from "./structuredEdit";

/**
 * Turn segmentation for the chat transcript.
 *
 * The transcript is a flat event stream: `derivePresentedChatRows` groups work
 * by adjacency and reads `turn_id` only to pair a call with its result, so the
 * unit an operator actually reviews — a turn — has no representation. This
 * module inserts a change summary after each turn that touched files.
 *
 * Turns are split on user messages, matching the backend's own rule: a
 * conversation turn record starts at a user request record
 * (`state/conversation_archive/turns.rs`). `turn_id` is deliberately *not*
 * used as the boundary because providers populate it inconsistently — Codex and
 * Gemini transcripts frequently omit it — and a missing id would silently merge
 * every turn into one.
 *
 * Counts come from the agent's own edit records, so they are claimed changes
 * rather than git-verified ones. That is the same distinction the Changes panel
 * draws with its `attributed` / `inferred` evidence field, and the card says so
 * rather than implying the working tree was inspected.
 */

export interface TurnChangeFile {
  path: string;
  added: number;
  removed: number;
  /** `created` only when a provider supplied whole-file content. */
  kind: "edited" | "created";
  /** True when line counts are unknown because only a path was reported. */
  counts_unknown: boolean;
}

export interface TurnChangeSummaryRow {
  kind: "turn_change_summary";
  id: string;
  files: TurnChangeFile[];
  added: number;
  removed: number;
}

export type ChatTranscriptRowModel = PresentedChatRow | TurnChangeSummaryRow;

function eventsOfRow(row: PresentedChatRow): AgentChatEvent[] {
  if (row.kind === "event") return [row.event];
  return row.entries.flatMap((entry) => [entry.primary_event, ...entry.merged_result_events]);
}

function isTurnBoundary(row: PresentedChatRow): boolean {
  return row.kind === "event" && row.event.kind === "message" && row.event.role === "user";
}

function mergeFile(files: Map<string, TurnChangeFile>, candidate: TurnChangeFile) {
  const existing = files.get(candidate.path);
  if (!existing) {
    files.set(candidate.path, { ...candidate });
    return;
  }
  existing.added += candidate.added;
  existing.removed += candidate.removed;
  // A later precise record supersedes an earlier path-only one, but a file
  // created in this turn stays created even if it is edited again after.
  existing.counts_unknown = existing.counts_unknown && candidate.counts_unknown;
  if (candidate.kind === "created") existing.kind = "created";
}

/**
 * Splits patch text into per-file counts.
 *
 * `diffStats` totals a whole patch, which is enough for a single tool row but
 * not for a card that lists files individually. Both patch dialects in use are
 * handled: git's `diff --git` headers and the `*** Update File:` headers Codex
 * emits from `apply_patch`.
 */
function patchChanges(patch: string): TurnChangeFile[] {
  const files: TurnChangeFile[] = [];
  let current: TurnChangeFile | null = null;

  patch.split(/\r\n|\r|\n/).forEach((line) => {
    const gitHeader = /^diff --git a\/(?:.+?) b\/(.+)$/.exec(line);
    const patchHeader = /^\*\*\* (Add|Update|Delete) File:\s+(.+)$/.exec(line);
    if (gitHeader || patchHeader) {
      current = {
        path: (gitHeader?.[1] ?? patchHeader?.[2] ?? "").trim(),
        added: 0,
        removed: 0,
        kind: patchHeader?.[1] === "Add" ? "created" : "edited",
        counts_unknown: false,
      };
      files.push(current);
      return;
    }
    if (!current) return;
    // `+++`/`---` are file headers rather than content in unified diffs.
    if (/^\+[^+]/.test(line)) current.added += 1;
    if (/^-[^-]/.test(line)) current.removed += 1;
  });

  return files.filter((file) => file.path.length > 0);
}

function collectTurnChanges(events: AgentChatEvent[]): TurnChangeFile[] {
  const files = new Map<string, TurnChangeFile>();

  events.forEach((event) => {
    const patch = toolPatchText(event) ?? (isDiffEvent(event) ? (event.text ?? "") : "");
    const patched = patch ? patchChanges(patch) : [];
    if (patched.length > 0) {
      patched.forEach((file) => mergeFile(files, file));
      return;
    }

    const edit = structuredEditFromEvent(event);
    if (edit?.file_path) {
      mergeFile(files, {
        path: edit.file_path,
        added: edit.added,
        removed: edit.removed,
        kind: edit.kind === "create" ? "created" : "edited",
        counts_unknown: false,
      });
      return;
    }

    // Providers that report only a path still prove the file was touched; the
    // card shows them without counts rather than inventing zeros.
    const written = writtenPaths(event);
    written.forEach((path) => {
      mergeFile(files, { path, added: 0, removed: 0, kind: "edited", counts_unknown: true });
    });
  });

  return [...files.values()];
}

/**
 * Paths this event proves were written. `changedPathsFromEvents` is
 * intentionally not used alone: it also matches paths a tool merely read, so a
 * write signal from the provider is required first.
 */
function writtenPaths(event: AgentChatEvent): string[] {
  const declared = event.metadata.files_written;
  if (Array.isArray(declared)) {
    const paths = declared.filter((value): value is string => typeof value === "string" && value.trim().length > 0);
    if (paths.length > 0) return paths;
  }
  if (isDiffEvent(event)) {
    return changedPathsFromEvents([event]);
  }
  return [];
}

function isDiffEvent(event: AgentChatEvent): boolean {
  return event.language === "diff" || /^diff --git |^\*\*\* (?:Add|Update|Delete) File:/m.test(event.text ?? "");
}

/**
 * Inserts a `turn_change_summary` row after every turn that touched files.
 * Turns that only talked produce no row.
 */
export function withTurnChangeSummaries(rows: PresentedChatRow[]): ChatTranscriptRowModel[] {
  const result: ChatTranscriptRowModel[] = [];
  let pending: PresentedChatRow[] = [];
  let turnOrdinal = 0;

  const flush = () => {
    if (pending.length === 0) return;
    const files = collectTurnChanges(pending.flatMap(eventsOfRow));
    if (files.length > 0) {
      result.push({
        kind: "turn_change_summary",
        id: `turn-change-${turnOrdinal}`,
        files,
        added: files.reduce((total, file) => total + file.added, 0),
        removed: files.reduce((total, file) => total + file.removed, 0),
      });
    }
    pending = [];
  };

  rows.forEach((row) => {
    if (isTurnBoundary(row)) {
      flush();
      turnOrdinal += 1;
    }
    result.push(row);
    pending.push(row);
  });

  flush();
  return result;
}

export function chatTranscriptRowKey(row: ChatTranscriptRowModel): string {
  if (row.kind === "event") return row.event.id;
  return row.id;
}
