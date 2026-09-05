import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { ListenersTable } from './ListenersTable';
import type { ListenerView } from '../../../types/automation';

const listener = (over: Partial<ListenerView> = {}): ListenerView => ({
  id: 'l1',
  blueprint_id: 'audit',
  name: 'Source audit',
  enabled: true,
  trigger: {
    type: 'file_watch',
    path: '/work/repo',
    recursive: true,
    patterns: ['**/*.rs'],
    ignore: [],
    events: [],
    debounce_ms: 500,
  },
  input: {},
  bindings: {},
  has_secret: false,
  runtime: {
    armed: true,
    fire_count: 3,
    recent_fire_epoch_ms: [],
    consecutive_failures: 0,
  },
  ...over,
});

describe('ListenersTable', () => {
  it('explains what a listener is when there are none', () => {
    render(
      <ListenersTable listeners={[]} onSetEnabled={vi.fn()} onRemove={vi.fn()} onEdit={vi.fn()} />,
    );
    expect(screen.getByText(/no listeners yet/i)).toBeInTheDocument();
  });

  it('renders what the listener watches and how often it has fired', () => {
    render(
      <ListenersTable
        listeners={[listener()]}
        onSetEnabled={vi.fn()}
        onRemove={vi.fn()}
        onEdit={vi.fn()}
      />,
    );
    expect(screen.getByTestId('listener-row-l1')).toBeInTheDocument();
    expect(screen.getByText('Source audit')).toBeInTheDocument();
    expect(screen.getByText('File')).toBeInTheDocument();
    expect(screen.getByText(/3 fires/)).toBeInTheDocument();
    expect(screen.getByText('Listening')).toBeInTheDocument();
  });

  it('warns that a non-poll listener misses events while the app is closed', () => {
    render(
      <ListenersTable
        listeners={[listener()]}
        onSetEnabled={vi.fn()}
        onRemove={vi.fn()}
        onEdit={vi.fn()}
      />,
    );
    expect(screen.getByText(/misses events while closed/)).toBeInTheDocument();
  });

  it('does not carry that warning for a poll listener, which recovers', () => {
    render(
      <ListenersTable
        listeners={[
          listener({
            trigger: {
              type: 'web_poll',
              url: 'https://example.invalid/releases',
              interval_seconds: 600,
              method: 'get',
              headers: {},
              fingerprint: 'etag_or_last_modified',
              json_pointer: null,
              regex: null,
              max_body_bytes: 1048576,
            },
          }),
        ]}
        onSetEnabled={vi.fn()}
        onRemove={vi.fn()}
        onEdit={vi.fn()}
      />,
    );
    expect(screen.queryByText(/misses events while closed/)).not.toBeInTheDocument();
  });

  it('toggles a listener through the enabled flag the user owns', () => {
    const onSetEnabled = vi.fn();
    render(
      <ListenersTable
        listeners={[listener()]}
        onSetEnabled={onSetEnabled}
        onRemove={vi.fn()}
        onEdit={vi.fn()}
      />,
    );
    fireEvent.click(screen.getByLabelText('Disable Source audit'));
    expect(onSetEnabled).toHaveBeenCalledWith('l1', false);
  });

  it('offers to re-enable a listener the rate ceiling stopped', () => {
    const onSetEnabled = vi.fn();
    render(
      <ListenersTable
        listeners={[
          listener({
            runtime: {
              armed: false,
              fire_count: 21,
              recent_fire_epoch_ms: [],
              consecutive_failures: 0,
              disabled_reason: 'auto-disabled after 21 fires in 60 seconds',
            },
          }),
        ]}
        onSetEnabled={onSetEnabled}
        onRemove={vi.fn()}
        onEdit={vi.fn()}
      />,
    );
    expect(screen.getByText('Auto-disabled')).toBeInTheDocument();
    expect(screen.getByText(/auto-disabled after 21 fires/)).toBeInTheDocument();
    // The user's switch was never flipped, so the control still reads
    // "Disable" and re-enabling goes through the explicit clear path.
    fireEvent.click(screen.getByLabelText('Disable Source audit'));
    expect(onSetEnabled).toHaveBeenCalledWith('l1', false);
  });

  it('only offers to copy a URL for a webhook listener', () => {
    const { rerender } = render(
      <ListenersTable
        listeners={[listener()]}
        onSetEnabled={vi.fn()}
        onRemove={vi.fn()}
        onEdit={vi.fn()}
      />,
    );
    expect(screen.queryByLabelText(/copy webhook url/i)).not.toBeInTheDocument();

    rerender(
      <ListenersTable
        listeners={[
          listener({
            trigger: {
              type: 'webhook',
              path_segment: 'ci',
              auth: 'hmac_sha256',
              signature_header: null,
              max_body_bytes: 262144,
            },
            webhook_url: 'http://127.0.0.1:8787/hooks/ci',
            has_secret: true,
          }),
        ]}
        onSetEnabled={vi.fn()}
        onRemove={vi.fn()}
        onEdit={vi.fn()}
      />,
    );
    expect(screen.getByLabelText(/copy webhook url/i)).toBeInTheDocument();
    expect(screen.getByText('/hooks/ci')).toBeInTheDocument();
  });
});
