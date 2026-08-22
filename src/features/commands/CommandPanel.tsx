import React, { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { writeText } from "@tauri-apps/plugin-clipboard-manager";
import { Check, Copy, MessageSquareCode, Send, Sparkles, Users } from "lucide-react";
import { useLibraryStore } from "../../store/useLibraryStore";
import { flattenAllEntries } from "../library/libraryListUtils";
import { AgentConfig, LibraryEntry } from "../../types";
import { useConfirm } from "../../components/ConfirmDialog";
import { DocsLink } from "../../components/DocsLink";
import { OnboardingHint } from "../../components/OnboardingHint";
import { flattenPromptForInjection, submitInputToAgents } from "../../utils/terminalInput";

interface CommandPanelProps {
  selectedAgentIds: Set<string>;
  broadcastMessage: string;
  setBroadcastMessage: (msg: string) => void;
  onBroadcast: (e: React.FormEvent) => void;
}

export const CommandPanel: React.FC<CommandPanelProps> = ({
  selectedAgentIds,
  broadcastMessage,
  setBroadcastMessage,
  onBroadcast,
}) => {
  const confirm = useConfirm();
  const index = useLibraryStore((s) => s.index);
  const fetchIndex = useLibraryStore((s) => s.fetchIndex);
  const [copiedPath, setCopiedPath] = useState<string | null>(null);
  const selectedAgentCount = selectedAgentIds.size;
  const targetLabel = selectedAgentCount > 0 ? `${selectedAgentCount} selected` : "All agents";

  useEffect(() => {
    if (!index) {
      void fetchIndex();
    }
  }, [index, fetchIndex]);

  // Quick prompts show only description/name from the index — the index is
  // metadata-only by design (see wardian-core::library::index), so the full
  // body is fetched on demand (inject/copy) via `read_library_item`.
  const quickPrompts = useMemo<LibraryEntry[]>(() => {
    const tree = index?.sections.prompts.tree;
    if (!tree) return [];
    return flattenAllEntries(tree)
      .map((row) => row.entry)
      .filter((entry): entry is LibraryEntry => entry != null && entry.is_starred);
  }, [index]);

  const readPromptContent = (path: string) => invoke<string>("read_library_item", { section: "prompts", path });

  const handleInject = async (path: string) => {
    try {
      const content = await readPromptContent(path);
      const flattenedPrompt = flattenPromptForInjection(content);
      if (selectedAgentIds.size > 0) {
        await submitInputToAgents(selectedAgentIds, flattenedPrompt);
      } else {
        if (await confirm("No agents selected. This will broadcast the prompt to all agents. Are you sure?")) {
          const agents = await invoke<AgentConfig[]>("list_agents");
          await submitInputToAgents(
            agents.map((agent) => agent.session_id),
            flattenedPrompt,
          );
        }
      }
    } catch (e) {
      console.error("Injection failed", e);
    }
  };

  const handleCopy = async (e: React.MouseEvent, path: string) => {
    e.stopPropagation();
    try {
      const content = await readPromptContent(path);
      await writeText(content);
      setCopiedPath(path);
      setTimeout(() => setCopiedPath(null), 2000);
    } catch (e) {
      console.error("Copy failed", e);
    }
  };

  const handleBroadcastSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (selectedAgentIds.size === 0) {
      if (!await confirm("No agents selected. This will broadcast to ALL agents. Are you sure?")) {
        return;
      }
    }
    onBroadcast(e);
  };

  return (
    <div data-testid="command-panel" className="flex h-full min-h-0 flex-col">
      <header className="mb-5 flex items-start justify-between gap-3">
        <div className="flex min-w-0 items-center gap-3">
          <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-lg border border-[var(--color-wardian-accent)]/30 bg-[var(--color-wardian-accent)]/10 text-[var(--color-wardian-accent)]">
            <MessageSquareCode className="h-4 w-4" aria-hidden="true" />
          </span>
          <div className="min-w-0">
            <h2 className="text-sm font-bold tracking-tight text-primary">Command</h2>
            <p className="mt-0.5 text-[11px] leading-4 text-muted-neutral">Send a prompt without leaving your work.</p>
          </div>
        </div>
        <span data-testid="command-target-scope" className="inline-flex shrink-0 items-center gap-1 rounded-full border border-wardian-border bg-wardian-card-bg-muted px-2 py-1 text-[10px] font-semibold text-muted-neutral">
          <Users className="h-3 w-3" aria-hidden="true" />
          {targetLabel}
        </span>
      </header>

      <div className="flex-1 overflow-y-auto pr-1 no-scrollbar">
        <div className="mb-5">
          <OnboardingHint
            id="command-targeting:v1"
            title="Target before you broadcast"
            actions={<DocsLink path="/guide/command-panel">Command guide</DocsLink>}
          >
            Select agents in the roster to limit a command. Sending with no selection intentionally asks for confirmation before reaching every agent.
          </OnboardingHint>
        </div>
        <section aria-labelledby="quick-prompts-heading">
          <div className="mb-3 flex items-center justify-between gap-2">
            <h3 id="quick-prompts-heading" className="text-xs font-bold tracking-wide text-muted">Quick Prompts</h3>
            <span className="rounded-full bg-wardian-card-bg-muted px-2 py-0.5 text-[10px] font-semibold text-muted-neutral">{quickPrompts.length} saved</span>
          </div>
          <div className="flex flex-col gap-2">
          {quickPrompts.length === 0 ? (
            <div className="rounded-lg border border-dashed border-wardian-border bg-wardian-card-bg-muted/60 px-3 py-4 text-center">
              <Sparkles className="mx-auto h-4 w-4 text-muted-neutral" aria-hidden="true" />
              <p className="mt-2 text-xs font-semibold text-primary">No quick prompts in Library.</p>
              <p className="mt-1 text-[11px] leading-4 text-muted-neutral">Star a Library prompt to keep a repeatable command close at hand.</p>
            </div>
          ) : (
            quickPrompts.map((prompt, idx) => (
              <div
                data-testid={`quick-prompt-${idx}`}
                key={`starred-${prompt.entry_ref}`}
                className="group/card relative"
              >
                <button
                  onClick={() => void handleInject(prompt.path)}
                  className="group flex w-full flex-col items-start rounded-lg border border-wardian-border bg-wardian-card-bg-muted px-3 py-2.5 pr-10 text-left text-primary transition-colors hover:border-[var(--color-wardian-accent)]/40 hover:bg-[var(--color-wardian-accent)]/5"
                >
                  <span className="w-full truncate text-xs font-bold group-hover:text-[var(--color-wardian-accent)]">{prompt.name}</span>
                  <span className="mt-1 w-full line-clamp-1 whitespace-pre-wrap text-[10px] leading-relaxed text-muted-neutral transition-colors group-hover:text-primary/70">
                    {prompt.description}
                  </span>
                </button>
                <button
                  onClick={(e) => void handleCopy(e, prompt.path)}
                  aria-label="Copy quick prompt to clipboard"
                  title="Copy to clipboard"
                  className={`absolute right-2 top-1/2 -translate-y-1/2 rounded-md border p-1.5 transition-all active:scale-95 ${
                    copiedPath === prompt.path
                      ? "bg-wardian-success/10 border-wardian-success/30 text-wardian-success"
                      : "border-transparent bg-wardian-card-bg text-muted-neutral hover:border-wardian-light hover:text-primary"
                  }`}
                >
                  {copiedPath === prompt.path ? (
                    <Check className="w-3 h-3" />
                  ) : (
                    <Copy className="w-3 h-3" />
                  )}
                </button>
              </div>
            ))
          )}
          </div>
        </section>
        </div>

      <section className="mt-4 shrink-0 border-t border-wardian-border pt-4" aria-labelledby="broadcast-heading">
        <div className="mb-3 flex items-center justify-between gap-2">
          <div>
            <h3 id="broadcast-heading" className="text-xs font-bold tracking-wide text-muted">Broadcast</h3>
            <p className="mt-0.5 text-[10px] text-muted-neutral">Deliver an ad-hoc instruction to {selectedAgentCount > 0 ? "your selection" : "every active agent"}.</p>
          </div>
          <Send className="h-3.5 w-3.5 text-muted-neutral" aria-hidden="true" />
        </div>
        <form onSubmit={handleBroadcastSubmit} className="flex flex-col gap-2">
          <textarea
            data-testid="broadcast-textarea"
            className="h-28 w-full resize-none rounded-lg border border-wardian-light bg-[var(--color-wardian-input-bg)] px-3 py-2.5 text-xs text-primary transition-colors placeholder:text-muted-neutral focus:border-[var(--color-wardian-accent)] focus:outline-none"
            placeholder={selectedAgentIds.size > 0 ? `Message ${selectedAgentIds.size} selected...` : "Broadcast to all agents..."}
            value={broadcastMessage}
            onChange={(e) => setBroadcastMessage(e.currentTarget.value)}
          />
          <button
            data-testid="broadcast-submit"
            type="submit"
            className="inline-flex h-9 items-center justify-center gap-1.5 rounded-lg border border-[var(--color-wardian-accent)]/40 bg-[var(--color-wardian-accent)] px-3 text-[11px] font-bold tracking-wide text-[var(--color-wardian-accent-contrast)] transition-colors hover:bg-[color-mix(in_srgb,var(--color-wardian-accent),white_12%)]"
          >
            <Send className="h-3.5 w-3.5" aria-hidden="true" />
            Execute Broadcast
          </button>
        </form>
      </section>
    </div>
  );
};
