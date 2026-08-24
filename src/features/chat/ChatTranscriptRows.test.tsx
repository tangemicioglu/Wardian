import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import type { AgentChatEvent } from '../../types';
import { ChatTranscriptRow } from './ChatTranscriptRows';

const memoryEvent: AgentChatEvent = {
  id: 'memory:event-1',
  session_id: 'agent-1',
  provider: 'wardian',
  kind: 'memory',
  role: 'system',
  text: '# Wardian memory\n\n- Prefer compact layouts',
  title: 'Memory loaded',
  status: 'succeeded',
  turn_id: null,
  source: 'wardian_memory',
  command: null,
  exit_code: null,
  path: null,
  language: 'markdown',
  created_at: '2026-08-23T12:00:00Z',
  sequence: 1,
  metadata: { memory_action: 'loaded' },
};

describe('memory transcript row', () => {
  it('is compact by default and reveals the exact injected context', () => {
    render(
      <ChatTranscriptRow
        agentIsWorking={false}
        isSubmitting={false}
        onApprovalSubmit={vi.fn()}
        row={{ kind: 'event', event: memoryEvent }}
      />,
    );

    expect(screen.getByText('Memory loaded')).toBeInTheDocument();
    expect(screen.queryByText('Prefer compact layouts')).toBeNull();

    fireEvent.click(screen.getByRole('button', { name: /Memory loaded/ }));
    expect(screen.getByText('Prefer compact layouts')).toBeInTheDocument();
  });
});
