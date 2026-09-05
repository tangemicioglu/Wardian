import { useState } from 'react';
import { Copy, Pencil, Power, PowerOff, Trash2 } from 'lucide-react';
import type { ListenerView } from '../../../types/automation';
import {
  lastFireLabel,
  listenerKindLabel,
  listenerProblemLabel,
  listenerSourceLabel,
  listenerStatusColor,
  listenerStatusLabel,
  survivesDowntime,
} from './listenerStatus';

interface ListenersTableProps {
  listeners: ListenerView[];
  onSetEnabled: (id: string, enabled: boolean) => void;
  onRemove: (id: string) => void;
  onEdit: (listener: ListenerView) => void;
}

const actionClass =
  'inline-flex h-7 w-7 cursor-pointer select-none items-center justify-center rounded border border-wardian-border text-muted hover:border-[var(--color-wardian-accent)] hover:text-[var(--color-wardian-accent)]';

export function ListenersTable({ listeners, onSetEnabled, onRemove, onEdit }: ListenersTableProps) {
  if (listeners.length === 0) {
    return (
      <div className="select-text rounded border border-dashed border-wardian-border p-4 text-center text-xs text-muted">
        No listeners yet - watch a folder, receive a webhook, or poll a URL to start an automation on an event.
      </div>
    );
  }

  return (
    <div className="select-text rounded border border-wardian-border">
      <table className="w-full table-fixed border-collapse text-left">
        <thead className="bg-[var(--color-wardian-card)] text-[10px] font-bold text-muted">
          <tr>
            <th scope="col" className="w-[104px] px-3 py-2">Status</th>
            <th scope="col" className="px-3 py-2">Automation</th>
            <th scope="col" className="w-[34%] px-3 py-2">Watching</th>
            <th scope="col" className="w-[18%] px-3 py-2">Last fire</th>
            <th scope="col" className="w-[132px] px-3 py-2 text-right">Actions</th>
          </tr>
        </thead>
        <tbody>
          {listeners.map((listener) => (
            <ListenerRow
              key={listener.id}
              listener={listener}
              onSetEnabled={onSetEnabled}
              onRemove={onRemove}
              onEdit={onEdit}
            />
          ))}
        </tbody>
      </table>
    </div>
  );
}

function ListenerRow({
  listener,
  onSetEnabled,
  onRemove,
  onEdit,
}: {
  listener: ListenerView;
  onSetEnabled: (id: string, enabled: boolean) => void;
  onRemove: (id: string) => void;
  onEdit: (listener: ListenerView) => void;
}) {
  const [copied, setCopied] = useState(false);
  const problem = listenerProblemLabel(listener);
  const source = listenerSourceLabel(listener.trigger);

  const copyWebhookUrl = async () => {
    if (!listener.webhook_url) return;
    try {
      await navigator.clipboard.writeText(listener.webhook_url);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      // Clipboard access can be refused; the URL stays visible in the row.
    }
  };

  return (
    <tr
      data-testid={`listener-row-${listener.id}`}
      className="select-text border-b border-wardian-border/70 bg-[var(--color-wardian-bg)] align-middle last:border-b-0 hover:bg-[color-mix(in_srgb,var(--color-wardian-card),transparent_45%)]"
    >
      <td className="w-[104px] px-3 py-2">
        <div className="flex items-center gap-2 text-[10px] font-bold text-muted">
          <span
            className="h-2 w-2 shrink-0 rounded-full"
            style={{ backgroundColor: listenerStatusColor(listener) }}
            aria-hidden
          />
          <span>{listenerStatusLabel(listener)}</span>
        </div>
      </td>
      <td className="min-w-0 px-3 py-2">
        <div className="truncate text-xs font-bold text-[var(--color-wardian-text)]" title={listener.name}>
          {listener.name}
        </div>
        <div className="mt-0.5 truncate text-[10px] text-muted" title={listener.blueprint_id}>
          {listener.blueprint_id}
        </div>
      </td>
      <td className="min-w-0 px-3 py-2">
        <div className="flex items-center gap-1.5">
          <span className="shrink-0 rounded border border-wardian-border px-1 text-[9px] font-bold uppercase text-muted">
            {listenerKindLabel(listener.trigger)}
          </span>
          <span className="truncate text-[10px] text-muted" title={source}>
            {source}
          </span>
        </div>
        {problem ? (
          <div className="mt-0.5 truncate text-[10px] text-[var(--color-wardian-error)]" title={problem}>
            {problem}
          </div>
        ) : null}
      </td>
      <td className="min-w-0 px-3 py-2">
        <div className="truncate text-[10px] text-muted">{lastFireLabel(listener)}</div>
        <div className="mt-0.5 truncate text-[10px] text-muted">
          {listener.runtime.fire_count} fires
          {survivesDowntime(listener.trigger) ? '' : ' · misses events while closed'}
        </div>
      </td>
      <td className="w-[132px] px-3 py-2 text-right">
        <div className="inline-flex shrink-0 items-center gap-1">
          {listener.webhook_url ? (
            <button
              type="button"
              className={actionClass}
              onClick={() => void copyWebhookUrl()}
              aria-label={`Copy webhook URL for ${listener.name}`}
              title={copied ? 'Copied' : listener.webhook_url}
            >
              <Copy size={13} />
            </button>
          ) : null}
          <button
            type="button"
            className={actionClass}
            onClick={() => onSetEnabled(listener.id, !listener.enabled)}
            aria-label={`${listener.enabled ? 'Disable' : 'Enable'} ${listener.name}`}
            title={listener.enabled ? 'Disable' : 'Enable'}
          >
            {listener.enabled ? <PowerOff size={13} /> : <Power size={13} />}
          </button>
          <button
            type="button"
            className={actionClass}
            onClick={() => onEdit(listener)}
            aria-label={`Edit ${listener.name}`}
            title="Edit"
          >
            <Pencil size={13} />
          </button>
          <button
            type="button"
            className={actionClass}
            onClick={() => onRemove(listener.id)}
            aria-label={`Remove ${listener.name}`}
            title="Remove"
          >
            <Trash2 size={13} />
          </button>
        </div>
      </td>
    </tr>
  );
}
