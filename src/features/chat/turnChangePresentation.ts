import type { TurnChangeFile } from "./chatTurns";

/**
 * Presentation rules for the turn change card.
 *
 * The collapsed card shows a few files as a preview. Picking the first N in
 * order tends to show N files from one directory, which says nothing about how
 * far the turn reached; picking one per top-level scope conveys breadth in the
 * same space.
 */

/**
 * The card expands on its own only while the whole list stays a glance rather
 * than a wall. Keyed on file count alone: the card lists paths, so line counts
 * do not affect how tall it gets.
 */
export const TURN_CHANGE_AUTO_EXPAND_FILE_LIMIT = 4;
export const TURN_CHANGE_PREVIEW_FILE_LIMIT = 3;
export const TURN_CHANGE_PREVIEW_SCOPE_LIMIT = 4;

export interface ChangeScopeSummary {
  label: string;
  file_count: number;
}

function pathSegments(value: string): string[] {
  return value
    .replace(/\\/g, "/")
    .split("/")
    .filter((segment) => segment.length > 0);
}

export function changedFileName(value: string): string {
  const segments = pathSegments(value);
  return segments[segments.length - 1] ?? value;
}

export function changedFileDirectory(value: string): string {
  const segments = pathSegments(value);
  return segments.length > 1 ? segments.slice(0, -1).join("/") : "";
}

/** Top-level directory, or `root` for a file at the workspace root. */
export function changeScope(value: string): string {
  const segments = pathSegments(value);
  return segments.length > 1 ? (segments[0] ?? "root") : "root";
}

export function shouldAutoExpandTurnChanges(files: readonly TurnChangeFile[]): boolean {
  return files.length > 0 && files.length <= TURN_CHANGE_AUTO_EXPAND_FILE_LIMIT;
}

/**
 * Ranks scopes by how many files each holds, breaking ties by first appearance
 * so the order is stable across renders of the same turn.
 */
export function summarizeChangeScopes(
  files: readonly TurnChangeFile[],
  limit = TURN_CHANGE_PREVIEW_SCOPE_LIMIT,
): ChangeScopeSummary[] {
  const scopes = new Map<string, { file_count: number; first_index: number }>();

  files.forEach((file, index) => {
    const label = changeScope(file.path);
    const current = scopes.get(label);
    scopes.set(label, {
      file_count: (current?.file_count ?? 0) + 1,
      first_index: current?.first_index ?? index,
    });
  });

  return [...scopes.entries()]
    .sort(
      ([leftLabel, left], [rightLabel, right]) =>
        right.file_count - left.file_count ||
        left.first_index - right.first_index ||
        leftLabel.localeCompare(rightLabel),
    )
    .slice(0, limit)
    .map(([label, scope]) => ({ label, file_count: scope.file_count }));
}

/** Picks preview files one per scope so the preview shows reach, not depth. */
export function selectChangePreview(
  files: readonly TurnChangeFile[],
  limit = TURN_CHANGE_PREVIEW_FILE_LIMIT,
): TurnChangeFile[] {
  const selected: TurnChangeFile[] = [];
  const seenScopes = new Set<string>();

  for (const file of files) {
    const scope = changeScope(file.path);
    if (seenScopes.has(scope)) continue;
    seenScopes.add(scope);
    selected.push(file);
    if (selected.length === limit) return selected;
  }

  // Fewer scopes than the limit: top up in order so the preview is not
  // artificially short when a turn worked inside a single directory.
  for (const file of files) {
    if (selected.length === limit) break;
    if (selected.includes(file)) continue;
    selected.push(file);
  }

  return selected;
}

export function sortTurnChangeFiles(files: readonly TurnChangeFile[]): TurnChangeFile[] {
  return [...files].sort((left, right) => left.path.localeCompare(right.path, undefined, { numeric: true }));
}
