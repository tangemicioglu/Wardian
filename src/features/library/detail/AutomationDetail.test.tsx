import { blueprintPage } from "../../../test/pageFixtures";
import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { AutomationDetail } from './AutomationDetail';
import { LibraryEntry } from '../../../types';

const mockInvoke = vi.mocked(invoke);

function automationEntry(overrides: Partial<LibraryEntry> = {}): LibraryEntry {
  return {
    kind: 'automation',
    path: 'a/foo.md',
    entry_ref: 'automations/a/foo.md',
    name: 'foo',
    description: '',
    tags: [],
    is_starred: false,
    deployment_count: 0,
    error: null,
    ...overrides,
  };
}

function renderAutomationDetail(entry: LibraryEntry = automationEntry()) {
  return render(
    <AutomationDetail
      entry={entry}
      header={<div />}
      draft="# foo"
      dirty={false}
      stale={false}
      onChange={vi.fn()}
      onSave={vi.fn()}
      onReloadExternal={vi.fn()}
      onKeepMine={vi.fn()}
    />,
  );
}

// MINOR: blueprint resolution used to match via `path.endsWith(entryPath)`
// on raw strings, which false-positives whenever an absolute path happens
// to end in the same substring as the entry's relative path without a real
// segment boundary (e.g. `.../other-a/foo.md` "ends with" `a/foo.md`).
describe('AutomationDetail blueprint resolution', () => {
  beforeEach(() => {
    mockInvoke.mockReset();
  });

  it('resolves via an exact trailing-segment match, ignoring a colliding endsWith substring', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'automation_list_blueprints') {
        return blueprintPage([
          // Colliding path: string-wise `endsWith('a/foo.md')` would match
          // this too, even though its real leaf folder is `other-a`, not `a`.
          { id: 'collision', name: 'collision', path: 'C:/workspace/other-a/foo.md' },
          { id: 'correct', name: 'correct', path: 'C:/workspace/automations/a/foo.md' },
        ]);
      }
      if (cmd === 'automation_parse') {
        return { blueprint: { schema: 1, id: 'correct', name: 'correct', nodes: [], edges: [] }, diagnostics: [] };
      }
      if (cmd === 'list_provider_readiness') return [];
      if (cmd === 'list_agents') return [];
      return null;
    });

    renderAutomationDetail();

    fireEvent.click(screen.getByTestId('automation-launch-run'));

    await waitFor(() =>
      expect(mockInvoke).toHaveBeenCalledWith('automation_parse', { path: 'C:/workspace/automations/a/foo.md' }),
    );
    expect(mockInvoke).not.toHaveBeenCalledWith('automation_parse', { path: 'C:/workspace/other-a/foo.md' });
    expect(screen.queryByTestId('automation-resolve-error')).not.toBeInTheDocument();
  });

  it('shows a resolve error when no ref has a real matching trailing segment', async () => {
    mockInvoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'automation_list_blueprints') {
        return blueprintPage([{ id: 'collision', name: 'collision', path: 'C:/workspace/other-a/foo.md' }]);
      }
      return null;
    });

    renderAutomationDetail();

    fireEvent.click(screen.getByTestId('automation-launch-run'));

    expect(await screen.findByTestId('automation-resolve-error')).toHaveTextContent(
      'Could not locate this automation file on disk.',
    );
    expect(mockInvoke).not.toHaveBeenCalledWith('automation_parse', expect.anything());
  });
});
