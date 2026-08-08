import { ChevronRight, FileDiff } from "lucide-react";
import { useState } from "react";

import { CopyIconButton } from "../grid/chatCopy";
import type { TurnChangeFile, TurnChangeSummaryRow } from "./chatTurns";
import {
  changedFileDirectory,
  changedFileName,
  selectChangePreview,
  shouldAutoExpandTurnChanges,
  sortTurnChangeFiles,
  summarizeChangeScopes,
} from "./turnChangePresentation";

/**
 * How each change kind is named to the reader.
 *
 * "Written" is distinct from "Created" on purpose: a write tool overwrites an
 * existing file as readily as it makes a new one, and the tool input does not
 * say which. Only a patch that shows the file appearing earns "Created".
 */
const FILE_KIND_LABELS: Record<TurnChangeFile["kind"], string> = {
  created: "Created",
  deleted: "Deleted",
  edited: "Edited",
  written: "Written",
};

const FILE_KIND_DOTS: Record<TurnChangeFile["kind"], string> = {
  created: "bg-[var(--color-wardian-success)]",
  deleted: "bg-[var(--color-wardian-error)]",
  edited: "bg-[var(--color-wardian-warning)]",
  written: "bg-[var(--color-wardian-warning)]",
};

/** Formats large counts compactly so the stat pair keeps a stable width. */
function formatDiffCount(value: number): string {
  if (value < 1000) return String(value);
  const thousands = value / 1000;
  return `${thousands < 10 ? thousands.toFixed(1).replace(/\.0$/, "") : Math.round(thousands)}k`;
}

export function DiffStat({ added, removed, className = "" }: { added: number; removed: number; className?: string }) {
  return (
    <span
      role="group"
      aria-label={`${added} additions, ${removed} deletions`}
      className={`inline-flex items-center gap-1 align-middle font-mono tabular-nums ${className}`}
    >
      <span aria-hidden="true" className="text-[var(--color-wardian-success)]">
        +{formatDiffCount(added)}
      </span>
      <span aria-hidden="true" className="text-[var(--color-wardian-error)]">
        -{formatDiffCount(removed)}
      </span>
    </span>
  );
}

function ChangeFileRow({
  file,
  onOpen,
}: {
  file: TurnChangeFile;
  onOpen?: (path: string) => void;
}) {
  const name = changedFileName(file.path);
  const directory = changedFileDirectory(file.path);
  const label = FILE_KIND_LABELS[file.kind];
  const content = (
    <>
      <span
        aria-label={`${label}, ${file.counts_unknown ? "line counts not reported" : "line counts reported"}`}
        className={`h-1.5 w-1.5 shrink-0 rounded-full ${FILE_KIND_DOTS[file.kind]} ${
          file.counts_unknown ? "opacity-50" : ""
        }`}
      />
      <span className="min-w-0 flex-1 truncate text-left text-[11px] leading-5">
        <span className="font-medium text-primary">{name}</span>
        {directory ? <span className="ml-1.5 text-muted-neutral">{directory}</span> : null}
      </span>
      {file.counts_unknown ? (
        <span className="shrink-0 font-mono text-[10px] text-muted-neutral">—</span>
      ) : (
        <DiffStat added={file.added} removed={file.removed} className="shrink-0 text-[10px]" />
      )}
    </>
  );

  if (!onOpen) {
    return (
      <div className="flex min-h-7 w-full items-center gap-2 px-2 py-1" title={file.path}>
        {content}
      </div>
    );
  }

  return (
    <button
      type="button"
      className="flex min-h-7 w-full items-center gap-2 px-2 py-1 text-left hover:bg-[var(--color-wardian-card-bg-muted)]"
      onClick={() => onOpen(file.path)}
      title={file.path}
    >
      {content}
    </button>
  );
}

/**
 * Summarizes what a turn changed, sitting after the turn's last row.
 *
 * Counts come from the agent's own edit records rather than a working-tree
 * diff, so the card says "reported by the agent" instead of implying the
 * repository was inspected. Files a provider named without line counts render
 * with an em dash rather than a fabricated zero.
 */
export function TurnChangeCard({
  row,
  onOpenFile,
}: {
  row: TurnChangeSummaryRow;
  onOpenFile?: (path: string) => void;
}) {
  const files = sortTurnChangeFiles(row.files);
  const [expanded, setExpanded] = useState(() => shouldAutoExpandTurnChanges(files));
  const previewFiles = selectChangePreview(files);
  const scopes = summarizeChangeScopes(files);
  const hasCounts = files.some((file) => !file.counts_unknown);
  // Without a user message to split on there is no turn, so the card must not
  // imply one; it names the span it actually covers.
  const fileLabel =
    row.scope === "whole_history"
      ? `${files.length} changed ${files.length === 1 ? "file" : "files"} in this transcript`
      : `${files.length} changed ${files.length === 1 ? "file" : "files"}`;

  return (
    <article
      className="rounded border border-wardian-light bg-[var(--color-wardian-card-bg-muted)]"
      data-testid="turn-change-card"
      data-expanded={expanded ? "true" : "false"}
    >
      <div className="flex items-center gap-2 px-2 py-1.5">
        <button
          type="button"
          aria-expanded={expanded}
          className="flex min-w-0 flex-1 items-center gap-1.5 text-left"
          onClick={() => setExpanded((value) => !value)}
        >
          <ChevronRight
            aria-hidden="true"
            className={`h-3.5 w-3.5 shrink-0 text-muted-neutral transition-transform ${expanded ? "rotate-90" : ""}`}
          />
          <FileDiff aria-hidden="true" className="h-3.5 w-3.5 shrink-0 text-muted-neutral" />
          <span className="flex min-w-0 items-center gap-1.5 whitespace-nowrap text-[12px] font-semibold leading-5 text-primary">
            {fileLabel}
            {hasCounts ? <DiffStat added={row.added} removed={row.removed} className="text-[11px]" /> : null}
          </span>
          {!expanded && scopes.length > 0 ? (
            <span className="ml-1 min-w-0 truncate text-[11px] text-muted-neutral">
              {scopes.map((scope) => `${scope.label} ${scope.file_count}`).join(" · ")}
            </span>
          ) : null}
        </button>
        <CopyIconButton label="Copy changed file paths" value={files.map((file) => file.path).join("\n")} />
      </div>

      {!expanded ? (
        <ul className="border-t border-wardian-light" data-testid="turn-change-preview">
          {previewFiles.map((file) => (
            <li key={file.path}>
              <ChangeFileRow file={file} onOpen={onOpenFile} />
            </li>
          ))}
          {files.length > previewFiles.length ? (
            <li className="px-2 py-1 text-[11px] leading-4 text-muted-neutral">
              +{files.length - previewFiles.length} more
            </li>
          ) : null}
        </ul>
      ) : (
        <ul className="border-t border-wardian-light" data-testid="turn-change-files">
          {files.map((file) => (
            <li key={file.path}>
              <ChangeFileRow file={file} onOpen={onOpenFile} />
            </li>
          ))}
        </ul>
      )}

      <p className="border-t border-wardian-light px-2 py-1 text-[10px] leading-4 text-muted-neutral">
        Reported by the agent, not a working-tree diff.
      </p>
    </article>
  );
}
