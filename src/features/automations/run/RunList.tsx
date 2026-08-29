import { ExternalLink } from 'lucide-react';
import type { RunSummary } from './runTypes';
import { formatRunStatus, automationRunStatusColor } from './statusLabels';

interface RunListProps {
  runs: RunSummary[];
  truncated?: boolean;
  selectedRunId: string | null;
  onOpen: (blueprintId: string, runId: string) => void;
  onLoadMore?: () => void;
  loadingMore?: boolean;
}

export function RunList({ runs, truncated = false, selectedRunId, onOpen, onLoadMore, loadingMore = false }: RunListProps) {
  if (runs.length === 0) {
    return (
      <div className="select-text rounded-lg border border-dashed border-wardian-border p-4 text-center text-xs text-[var(--color-wardian-text-muted)]">
        No runs yet.
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-2">
      {truncated && (
        <div className="rounded border border-[var(--color-wardian-warning)]/40 bg-[var(--color-wardian-warning)]/10 px-2 py-1.5 text-[11px] text-[var(--color-wardian-warning)]" role="status">
          <span>Showing the 200 most recent runs; pages are capped at 200.</span>{' '}
          {onLoadMore && (
            <button type="button" className="font-semibold underline disabled:opacity-50" onClick={onLoadMore} disabled={loadingMore}>
              {loadingMore ? 'Loading…' : 'Load next 200'}
            </button>
          )}
        </div>
      )}
      {runs.map((run) => {
        const color = automationRunStatusColor(run.status);
        const selected = selectedRunId === run.run_id;
        return (
          <div
            key={`${run.blueprint_id}:${run.run_id}`}
            className={`w-full rounded-lg border p-3 transition-colors ${
              selected
                ? 'border-[var(--color-wardian-accent)] bg-[color-mix(in_srgb,var(--color-wardian-accent),transparent_90%)]'
                : 'border-wardian-border bg-[var(--color-wardian-card)]'
            }`}
          >
            <div className="flex items-start justify-between gap-3">
              <div className="min-w-0 select-text">
                <div className="truncate text-xs font-bold text-[var(--color-wardian-text)]">{run.run_id}</div>
                <div className="mt-1 truncate text-[10px] font-mono text-[var(--color-wardian-text-muted)]">{run.blueprint_id}</div>
              </div>
              <div className="flex shrink-0 items-center gap-2">
                <span
                  className="rounded border px-1.5 py-0.5 text-[9px] font-bold"
                  style={{
                    borderColor: color,
                    color,
                    backgroundColor: `color-mix(in srgb, ${color}, transparent 86%)`,
                  }}
                >
                  {formatRunStatus(run.status)}
                </span>
                <button
                  type="button"
                  aria-label={`Open ${run.blueprint_id} run ${run.run_id}`}
                  title="Open run"
                  onClick={() => onOpen(run.blueprint_id, run.run_id)}
                  className="inline-flex h-7 items-center gap-1 rounded border border-wardian-border px-2 text-[10px] font-bold text-muted transition-colors hover:border-[var(--color-wardian-accent)] hover:text-[var(--color-wardian-accent)] cursor-pointer select-none"
                >
                  <ExternalLink className="h-3.5 w-3.5" aria-hidden />
                  Open
                </button>
              </div>
            </div>
            <div className="mt-2 flex select-text items-center justify-between gap-2 text-[10px] text-[var(--color-wardian-text-muted)]">
              <span>{run.node_count} nodes</span>
              {run.failure ? <span className="truncate text-[var(--color-wardian-error)]">{run.failure}</span> : null}
            </div>
          </div>
        );
      })}
    </div>
  );
}
