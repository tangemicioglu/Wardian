import React, { useState } from "react";
import { Bot, ChevronDown, ChevronUp, GitBranch, Inbox, Terminal } from "lucide-react";
import type { QueueItem } from "../../types";
import { useRemoteStore } from "./useRemoteStore";
import { QUEUE_TONE_CLASSES, queueItemIsAgentEvent, queueItemLabel, queueItemTone } from "../queue/queuePresentation";

function relativeTime(ts: number): string {
  const diffMs = Date.now() - ts;
  const secs = Math.floor(diffMs / 1000);
  if (secs < 60) return "just now";
  const mins = Math.floor(secs / 60);
  if (mins < 60) return `${mins}m ago`;
  const hrs = Math.floor(mins / 60);
  if (hrs < 24) return `${hrs}h ago`;
  return `${Math.floor(hrs / 24)}d ago`;
}

function RemoteInboxCard({ item, onOpenAgent }: { item: QueueItem; onOpenAgent: (sessionId: string) => void }) {
  const [isExpanded, setIsExpanded] = useState(false);
  const title = item.notification_title ?? item.agent_name ?? item.workflow_name ?? "Unknown";
  const bodyText = item.status === "failed" && item.error ? item.error : item.summary;
  const Icon = queueItemIsAgentEvent(item) ? Bot : GitBranch;
  const classes = QUEUE_TONE_CLASSES[queueItemTone(item)];
  const isExpandable = Boolean(bodyText && (bodyText.length > 220 || bodyText.split("\n").length > 4));
  const summaryId = `remote-queue-item-summary-${item.id}`;
  const canOpenAgent = Boolean(item.agent_session_id);

  return (
    <article
      className={`relative overflow-hidden rounded-md border px-3 py-3 ${
        item.read ? "border-wardian-border bg-wardian-card-bg-muted" : "border-[var(--color-wardian-accent)]/30 bg-wardian-card-bg"
      }`}
    >
      <div className={`absolute left-0 top-0 h-full w-1 ${classes.accent}`} />
      {!item.read && (
        <span
          data-testid="remote-queue-unread-dot"
          className="absolute left-2 top-2 z-10 h-2 w-2 rounded-full bg-[var(--color-wardian-accent)] shadow-[0_0_0_2px_var(--color-wardian-bg)]"
        />
      )}
      <div className="flex items-start gap-3 pl-2">
        <span
          className={`mt-0.5 flex h-8 w-8 shrink-0 items-center justify-center rounded-md ${classes.icon}`}
          aria-hidden="true"
        >
          <Icon className="h-4 w-4" />
        </span>
        <div className="min-w-0 flex-1">
          <div className="flex min-w-0 flex-wrap items-center gap-2">
            <span className="truncate text-sm font-semibold text-primary">{title}</span>
            <span className={`rounded-full px-2 py-0.5 text-[10px] font-bold ${classes.badge}`}>
              {queueItemLabel(item)}
            </span>
            <span className="shrink-0 text-[10px] text-muted-neutral">{relativeTime(item.timestamp)}</span>
          </div>
          {bodyText && (
            <div className="mt-2 space-y-2">
              <p
                id={summaryId}
                data-testid={summaryId}
                className={`whitespace-pre-wrap break-words text-[13px] leading-5 text-muted ${
                  isExpandable && !isExpanded
                    ? "line-clamp-4"
                    : isExpandable
                      ? "max-h-80 overflow-y-auto pr-2"
                      : ""
                }`}
              >
                {bodyText}
              </p>
              {isExpandable && (
                <button
                  type="button"
                  aria-controls={summaryId}
                  aria-expanded={isExpanded}
                  aria-label={isExpanded ? "Collapse summary" : "Show full summary"}
                  onClick={() => setIsExpanded((value) => !value)}
                  className="inline-flex items-center gap-1 rounded-md text-[11px] font-semibold text-muted-neutral transition-colors hover:text-bright-neutral"
                >
                  {isExpanded ? (
                    <ChevronUp className="h-3 w-3" aria-hidden="true" />
                  ) : (
                    <ChevronDown className="h-3 w-3" aria-hidden="true" />
                  )}
                  {isExpanded ? "Hide details" : "Show details"}
                </button>
              )}
            </div>
          )}
          {canOpenAgent && (
            <div className="mt-3">
              <button
                type="button"
                aria-label="Open agent terminal"
                title="Open agent terminal"
                onClick={() => onOpenAgent(item.agent_session_id!)}
                className="inline-flex h-7 items-center gap-1 rounded-md border border-wardian-border bg-wardian-card-bg-muted px-2 text-[11px] font-semibold text-muted-neutral transition-colors hover:text-bright-neutral"
              >
                <Terminal className="h-3.5 w-3.5" aria-hidden="true" />
                Open agent
              </button>
            </div>
          )}
        </div>
      </div>
    </article>
  );
}

export const RemoteInboxView: React.FC = () => {
  const items = useRemoteStore((state) => state.remoteQueueItems);
  const openAgent = useRemoteStore((state) => state.openAgent);

  return (
    <section className="flex min-h-0 flex-1 flex-col">
      <header className="shrink-0 border-b border-wardian-border bg-wardian-bg/95 px-4 py-3 backdrop-blur">
        <div className="flex items-center gap-2">
          <Inbox className="h-4 w-4 text-muted-neutral" aria-hidden="true" />
          <h1 className="truncate text-base font-semibold text-primary">Inbox</h1>
        </div>
      </header>
      <div className="min-h-0 flex-1 overflow-y-auto px-3 py-4">
        {items.length === 0 ? (
          <div className="rounded-md border border-dashed border-wardian-border px-3 py-4 text-xs text-muted-neutral">
            No Inbox items.
          </div>
        ) : (
          <div className="flex flex-col gap-3">
            {items.map((item) => (
              <RemoteInboxCard key={item.id} item={item} onOpenAgent={(sessionId) => void openAgent(sessionId)} />
            ))}
          </div>
        )}
      </div>
    </section>
  );
};
