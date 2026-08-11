import type {
  ChangeReviewBaseline,
  ChangeReviewFileEntry,
  FilesComparisonBaseline,
  FilesSurfaceStateV2,
} from "../../types";

/**
 * How a change-review entry becomes an open comparison.
 *
 * Extracted from `ChangesPanel` so the Garden's terrain can open a changed file
 * the same way the sidebar does. Two surfaces opening one change set through two
 * code paths would drift on exactly the details that matter — which revision a
 * rename compares against, whether an added file is marked absent — and the
 * operator would learn the difference by reading the wrong diff.
 */

export const CHANGE_BASELINE_OPTIONS: readonly {
  value: ChangeReviewBaseline;
  label: string;
}[] = [
  { value: "last_effective_turn", label: "Last turn" },
  { value: "conversation_start", label: "This conversation" },
  { value: "branch_point", label: "This branch" },
  { value: "head", label: "Last commit" },
  { value: "unreviewed", label: "I last looked" },
];

export const DEFAULT_CHANGE_REVIEW_BASELINE: ChangeReviewBaseline = "last_effective_turn";

export function isChangeReviewBaseline(value: unknown): value is ChangeReviewBaseline {
  return CHANGE_BASELINE_OPTIONS.some((option) => option.value === value);
}

export function changeBaselineLabel(baseline: ChangeReviewBaseline): string {
  return CHANGE_BASELINE_OPTIONS.find((option) => option.value === baseline)?.label ?? "Baseline";
}

/** Absolute path for a change-review entry, which git reports workspace-relative. */
export function pathForWorkspace(workspace: string, path: string): string {
  if (/^[A-Za-z]:[\\/]/.test(path) || path.startsWith("/") || path.startsWith("\\\\")) {
    return path;
  }
  return `${workspace.replace(/[\\/]+$/g, "")}/${path.replace(/^[\\/]+/g, "")}`;
}

/**
 * Builds the comparison baseline for a change-review entry.
 *
 * `old_path` is used when present so a renamed file is compared against the name
 * it had at the baseline. Added and untracked files are marked `absent` rather
 * than pointed at a revision that does not contain them.
 */
export function baselineForFile(
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
export function changeSurfaceState(baseline: FilesComparisonBaseline): FilesSurfaceStateV2 {
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
