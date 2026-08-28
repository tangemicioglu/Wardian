import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';

const invokeMock = vi.fn();

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

import { BlueprintSelector } from './BlueprintSelector';

/** `workflow_list_blueprints` returns a page, never a bare array. */
const blueprintPage = (blueprints: unknown[], next: number | null = null) => ({
  blueprints,
  truncated: next !== null,
  next_offset: next,
});

describe('BlueprintSelector', () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it('lists blueprints from workflow_list_blueprints and opens one', async () => {
    invokeMock.mockResolvedValueOnce(blueprintPage([{ id: 'wf', name: 'WF', path: '/x/wf.md' }]));
    const onOpen = vi.fn();

    render(<BlueprintSelector onOpen={onOpen} onNew={() => {}} />);

    await waitFor(() => expect(screen.getByText('WF')).toBeInTheDocument());
    fireEvent.change(screen.getByRole('combobox'), { target: { value: '/x/wf.md' } });

    expect(onOpen).toHaveBeenCalledWith('/x/wf.md');
  });

  it('fires onNew', async () => {
    invokeMock.mockResolvedValueOnce(blueprintPage([]));
    const onNew = vi.fn();

    render(<BlueprintSelector onOpen={() => {}} onNew={onNew} />);
    await waitFor(() => expect(invokeMock).toHaveBeenCalledWith('workflow_list_blueprints'));
    fireEvent.click(screen.getByRole('button', { name: /new/i }));

    expect(onNew).toHaveBeenCalled();
  });

  it('marks a partial workflow catalog', async () => {
    invokeMock.mockResolvedValueOnce({
      blueprints: [{ id: 'wf', name: 'WF', path: '/x/wf.md' }],
      truncated: true,
    });

    render(<BlueprintSelector onOpen={() => {}} onNew={() => {}} />);

    expect(await screen.findByRole('status')).toHaveTextContent('first 500');
  });

  it('loads one more bounded catalog page', async () => {
    invokeMock
      .mockResolvedValueOnce({ blueprints: [{ id: 'wf-1', name: 'WF 1', path: '/x/1.md' }], truncated: true, next_offset: 500 })
      .mockResolvedValueOnce({ blueprints: [{ id: 'wf-2', name: 'WF 2', path: '/x/2.md' }], truncated: false, next_offset: null });

    render(<BlueprintSelector onOpen={() => {}} onNew={() => {}} />);
    fireEvent.click(await screen.findByRole('button', { name: /load next 500/i }));

    await waitFor(() => expect(screen.getByText('WF 2')).toBeInTheDocument());
    expect(invokeMock).toHaveBeenLastCalledWith('workflow_list_blueprints', { offset: 500 });
  });
});
