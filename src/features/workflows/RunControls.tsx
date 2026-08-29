import { invoke } from '@tauri-apps/api/core';
import { useState } from 'react';

export type RunControlStatus = 'running' | 'awaiting_approval' | 'completed' | 'failed' | 'interrupted';

interface RunControlsProps {
  blueprintId: string;
  runId: string;
  blueprintPath: string;
  status: RunControlStatus;
  awaitingNode: string | null;
  onChanged: () => void;
}

export function RunControls({
  blueprintId,
  runId,
  blueprintPath,
  status,
  awaitingNode,
  onChanged,
}: RunControlsProps) {
  const [pendingAction, setPendingAction] = useState<'approve' | 'reject' | 'resume' | 'cancel' | null>(null);
  const [error, setError] = useState<string | null>(null);

  const call = async (cmd: string, args: Record<string, unknown>) => {
    const action = cmd === 'workflow_approve'
      ? args.granted === true ? 'approve' : 'reject'
      : cmd === 'workflow_resume' ? 'resume' : 'cancel';
    setError(null);
    setPendingAction(action);
    try {
      await invoke(cmd, args);
      onChanged();
    } catch (cause) {
      const detail = cause instanceof Error ? cause.message : String(cause);
      setError(`Could not ${action} this workflow run: ${detail}`);
    } finally {
      setPendingAction(null);
    }
  };

  return (
    <div className="run-controls flex flex-col items-end gap-1" data-testid="run-controls">
      <div className="flex gap-2">
        {status === 'awaiting_approval' && awaitingNode && (
          <>
          <button
            type="button"
            className="cursor-pointer rounded bg-[var(--color-wardian-success)] px-2 py-1 text-xs text-[var(--color-wardian-bg)] disabled:cursor-not-allowed disabled:opacity-50"
            disabled={pendingAction !== null}
            onClick={() =>
              call('workflow_approve', {
                blueprintId,
                runId,
                blueprintPath,
                node: awaitingNode,
                granted: true,
                actor: 'user',
                note: null,
              })
            }
          >
            {pendingAction === 'approve' ? 'Approving…' : 'Approve'}
          </button>
          <button
            type="button"
            className="cursor-pointer rounded bg-[var(--color-wardian-error)] px-2 py-1 text-xs text-[var(--color-wardian-bg)] disabled:cursor-not-allowed disabled:opacity-50"
            disabled={pendingAction !== null}
            onClick={() =>
              call('workflow_approve', {
                blueprintId,
                runId,
                blueprintPath,
                node: awaitingNode,
                granted: false,
                actor: 'user',
                note: null,
              })
            }
          >
            {pendingAction === 'reject' ? 'Rejecting…' : 'Reject'}
          </button>
          <button
            type="button"
            className="rounded border border-wardian-border px-2 py-1 text-xs text-primary disabled:cursor-not-allowed disabled:opacity-50"
            disabled={pendingAction !== null}
            onClick={() => call('workflow_cancel', { blueprintId, runId })}
          >
            {pendingAction === 'cancel' ? 'Cancelling…' : 'Cancel'}
          </button>
          </>
        )}
        {status === 'interrupted' && (
        <button
          type="button"
          className="rounded border border-wardian-border px-2 py-1 text-xs text-primary"
          disabled={pendingAction !== null}
          onClick={() => call('workflow_resume', { blueprintId, runId, blueprintPath })}
        >
          {pendingAction === 'resume' ? 'Resuming…' : 'Resume'}
        </button>
        )}
        {status === 'running' && (
        <button
          type="button"
          className="rounded border border-wardian-border px-2 py-1 text-xs text-primary"
          disabled={pendingAction !== null}
          onClick={() => call('workflow_cancel', { blueprintId, runId })}
        >
          {pendingAction === 'cancel' ? 'Cancelling…' : 'Cancel'}
        </button>
        )}
      </div>
      {error ? <p role="alert" className="max-w-[280px] text-right text-[10px] text-[var(--color-wardian-error)]">{error}</p> : null}
    </div>
  );
}
