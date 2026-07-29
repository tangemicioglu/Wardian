/** Shared Git status wording and semantic color tokens for Explorer and Source Control. */
const GIT_STATUS_PRESENTATION: Record<string, { color: string; label: string; textClass: string }> = {
  M: { color: "var(--color-wardian-warning)", label: "Modified", textClass: "text-[var(--color-wardian-warning)]" },
  A: { color: "var(--color-wardian-success)", label: "Added", textClass: "text-[var(--color-wardian-success)]" },
  D: { color: "var(--color-wardian-error)", label: "Deleted", textClass: "text-[var(--color-wardian-error)]" },
  R: { color: "var(--color-wardian-processing)", label: "Renamed", textClass: "text-[var(--color-wardian-processing)]" },
  C: { color: "var(--color-wardian-processing)", label: "Copied", textClass: "text-[var(--color-wardian-processing)]" },
  U: { color: "var(--color-wardian-warning)", label: "Unmerged", textClass: "text-[var(--color-wardian-warning)]" },
  AA: { color: "var(--color-wardian-warning)", label: "Both Added", textClass: "text-[var(--color-wardian-warning)]" },
  AU: { color: "var(--color-wardian-warning)", label: "Added By Us", textClass: "text-[var(--color-wardian-warning)]" },
  DD: { color: "var(--color-wardian-error)", label: "Both Deleted", textClass: "text-[var(--color-wardian-error)]" },
  DU: { color: "var(--color-wardian-error)", label: "Deleted By Us", textClass: "text-[var(--color-wardian-error)]" },
  UA: { color: "var(--color-wardian-warning)", label: "Added By Them", textClass: "text-[var(--color-wardian-warning)]" },
  UD: { color: "var(--color-wardian-error)", label: "Deleted By Them", textClass: "text-[var(--color-wardian-error)]" },
  UU: { color: "var(--color-wardian-warning)", label: "Both Modified", textClass: "text-[var(--color-wardian-warning)]" },
  "?": { color: "var(--color-wardian-success)", label: "Untracked", textClass: "text-[var(--color-wardian-success)]" },
};

export function gitStatusColor(status: string | undefined): string {
  return GIT_STATUS_PRESENTATION[status ?? ""]?.color ?? "var(--color-wardian-text-muted)";
}

export function gitStatusTextClass(status: string | undefined): string {
  return GIT_STATUS_PRESENTATION[status ?? ""]?.textClass ?? "text-muted";
}

export function gitStatusLabel(status: string): string {
  return GIT_STATUS_PRESENTATION[status]?.label ?? status;
}
