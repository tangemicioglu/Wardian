import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const invokeMock = vi.fn();

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import { RunControls } from './RunControls';

const base = {
  blueprintId: 'wf',
  runId: 'run-1',
  blueprintPath: '/x/wf.md',
  status: 'running' as const,
  awaitingNode: null,
};

describe('RunControls', () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it('cancels a running run', async () => {
    invokeMock.mockResolvedValueOnce({ ok: true });

    render(<RunControls {...base} onChanged={() => {}} />);
    fireEvent.click(screen.getByRole('button', { name: /cancel/i }));

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('automation_cancel', { blueprintId: 'wf', runId: 'run-1' });
    });
  });

  it('shows resume for an interrupted run', () => {
    render(<RunControls {...base} status="interrupted" onChanged={() => {}} />);

    expect(screen.getByRole('button', { name: /resume/i })).toBeInTheDocument();
  });

  it('shows approve/reject when awaiting approval', async () => {
    invokeMock.mockResolvedValueOnce({ ok: true });
    const onChanged = vi.fn();

    render(<RunControls {...base} status="awaiting_approval" awaitingNode="gate" onChanged={onChanged} />);
    fireEvent.click(screen.getByRole('button', { name: /approve/i }));

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith(
        'automation_approve',
        expect.objectContaining({ blueprintId: 'wf', runId: 'run-1', node: 'gate', granted: true, note: null }),
      );
      expect(onChanged).toHaveBeenCalledOnce();
    });
  });

  it('cancels an approval-parked run', async () => {
    invokeMock.mockResolvedValueOnce({ ok: true });

    render(<RunControls {...base} status="awaiting_approval" awaitingNode="gate" onChanged={() => {}} />);
    fireEvent.click(screen.getByRole('button', { name: /^cancel$/i }));

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith('automation_cancel', { blueprintId: 'wf', runId: 'run-1' });
    });
  });

  it('keeps the approval visible and reports a backend failure', async () => {
    invokeMock.mockRejectedValueOnce(new Error('run is no longer awaiting approval'));
    const onChanged = vi.fn();

    render(<RunControls {...base} status="awaiting_approval" awaitingNode="gate" onChanged={onChanged} />);
    fireEvent.click(screen.getByRole('button', { name: /^reject$/i }));

    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Could not reject this automation run: run is no longer awaiting approval',
    );
    expect(onChanged).not.toHaveBeenCalled();
    expect(screen.getByRole('button', { name: /^approve$/i })).toBeEnabled();
  });
});
