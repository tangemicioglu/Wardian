import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { FilePlus2, FileText, Loader2, SendHorizontal, X } from "lucide-react";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { KeyboardEvent } from "react";
import type { AgentChatEvent, AgentConfig, AgentTelemetry } from "../../types";
import { useSettingsStore } from "../../store/useSettingsStore";
import { reasoningEffortForConfig } from "../agents/configUtils";
import { ProviderModelSelector, type ModelSelection } from "../agents/ProviderModelSelector";
import {
  promptWithChatAttachments,
  stageChatImageAttachments,
  submitInputToAgent,
  type ChatAttachment,
} from "../../utils/terminalInput";
import { ChatTranscriptRow } from "../chat/ChatTranscriptRows";
import {
  isProcessingAgentStatus,
  liveApprovalEventId,
  shouldShowChatEvent,
  sortTranscriptEvents,
} from "../chat/chatPresentation";
import { chatTranscriptRowKey, withTurnChangeSummaries, type ChatTranscriptRowModel } from "../chat/chatTurns";
import { useAppShellWorkbenchNavigation } from "../../layout/AppShell";
import { fileResourceKey } from "../files/fileResourceKey";
import { type ChatMarkdownLinkHandling } from "./markdown/ChatMarkdown";
import { derivePresentedChatRows } from "./workLogPresentation";

interface AgentChatViewBaseProps {
  sessionId: string;
  agent?: Pick<AgentConfig, "session_name" | "agent_class" | "provider" | "model" | "provider_config">;
  provider?: AgentConfig["provider"];
  isMaximized?: boolean;
  theme?: "dark" | "light" | "system";
  status?: string | null;
  telemetry?: Pick<AgentTelemetry, "current_status"> | null;
  className?: string;
  workspacePath?: string | null;
  refreshIntervalMs?: number;
  autoFocusComposer?: boolean;
  onComposerAutoFocused?: () => void;
}

type AgentChatDraftControlProps =
  | { draft?: undefined; onDraftChange?: undefined }
  | { draft: string; onDraftChange: (value: string) => void };

type AgentChatViewProps = AgentChatViewBaseProps & AgentChatDraftControlProps;

type LoadState = "loading" | "ready" | "error";
const CHAT_REFRESH_INTERVAL_MS = 3000;


type AwaitingResponseMarker = { id: string; response_count_after: number };

const CHAT_INITIAL_ROW_LIMIT = 80;
const CHAT_ROW_PAGE_SIZE = 60;
const CHAT_SCROLL_BOTTOM_THRESHOLD_PX = 48;

export function AgentChatView({
  sessionId,
  agent,
  provider,
  isMaximized = false,
  theme = "system",
  status,
  telemetry,
  className = "",
  workspacePath,
  refreshIntervalMs = CHAT_REFRESH_INTERVAL_MS,
  autoFocusComposer = false,
  draft,
  onComposerAutoFocused,
  onDraftChange,
}: AgentChatViewProps) {
  const [events, setEvents] = useState<AgentChatEvent[]>([]);
  const [pendingMessages, setPendingMessages] = useState<AgentChatEvent[]>([]);
  const [awaitingResponse, setAwaitingResponse] = useState<AwaitingResponseMarker | null>(null);
  const [loadState, setLoadState] = useState<LoadState>("loading");
  const [error, setError] = useState<string | null>(null);
  const [reloadKey, setReloadKey] = useState(0);
  const [internalDraft, setInternalDraft] = useState("");
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const [attachments, setAttachments] = useState<ChatAttachment[]>([]);
  const [visibleRowLimit, setVisibleRowLimit] = useState(CHAT_INITIAL_ROW_LIMIT);
  const workbenchNavigation = useAppShellWorkbenchNavigation();
  const externalEditor = useSettingsStore((state) => state.externalEditor);
  const externalEditorCustomExecutable = useSettingsStore((state) => state.externalEditorCustomExecutable);
  const transcriptScrollRef = useRef<HTMLDivElement>(null);
  const transcriptRequestRef = useRef(0);
  const stickToLatestRef = useRef(true);
  const prependScrollSnapshotRef = useRef<{ scrollHeight: number; scrollTop: number } | null>(null);
  const activeDraft = draft ?? internalDraft;
  const setActiveDraft = onDraftChange ?? setInternalDraft;

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;

    listen<{ session_id?: string }>("agent-terminal-cleared", (event) => {
      if (event.payload?.session_id !== sessionId) return;
      transcriptRequestRef.current += 1;
      stickToLatestRef.current = true;
      prependScrollSnapshotRef.current = null;
      setEvents([]);
      setPendingMessages([]);
      setAwaitingResponse(null);
      setLoadState("ready");
      setError(null);
      setSubmitError(null);
      setAttachments([]);
    })
      .then((dispose) => {
        if (disposed) {
          dispose();
          return;
        }
        unlisten = dispose;
      })
      .catch((reason) => {
        console.warn("agent-terminal-cleared chat listener error:", reason);
      });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [sessionId]);

  useEffect(() => {
    let cancelled = false;
    let intervalId: number | null = null;

    const loadTranscript = (showLoading: boolean) => {
      if (!showLoading && document.visibilityState === "hidden") return;
      const requestId = ++transcriptRequestRef.current;
      if (showLoading) {
        setLoadState("loading");
        setError(null);
      }

      invoke<AgentChatEvent[]>("load_agent_chat_transcript", { sessionId })
        .then((transcript) => {
          if (cancelled || requestId !== transcriptRequestRef.current) return;
          const nextEvents = Array.isArray(transcript) ? transcript : [];
          const scrollRegion = transcriptScrollRef.current;
          if (scrollRegion && !prependScrollSnapshotRef.current) {
            stickToLatestRef.current = stickToLatestRef.current || isNearTranscriptBottom(scrollRegion);
          }
          setEvents(nextEvents);
          setPendingMessages((pending) => unconfirmedPendingMessages(nextEvents, pending));
          setAwaitingResponse((marker) => clearAwaitingResponseWhenAnswered(nextEvents, marker));
          setLoadState("ready");
          setError(null);
        })
        .catch((reason: unknown) => {
          if (cancelled || requestId !== transcriptRequestRef.current || !showLoading) return;
          setEvents([]);
          setError(errorMessage(reason));
          setLoadState("error");
        });
    };

    loadTranscript(true);
    intervalId = window.setInterval(() => loadTranscript(false), refreshIntervalMs);

    return () => {
      cancelled = true;
      if (intervalId !== null) window.clearInterval(intervalId);
    };
  }, [sessionId, reloadKey, refreshIntervalMs]);

  const mergedEvents = useMemo(() => mergePendingMessages(events, pendingMessages), [events, pendingMessages]);
  const activeStatus = status ?? telemetry?.current_status ?? null;
  const showThinking = isProcessingAgentStatus(activeStatus) || awaitingResponse !== null || pendingMessages.length > 0;
  const displayEvents = useMemo(
    () =>
      appendThinkingIndicator(
        mergedEvents,
        sessionId,
        agent?.provider ?? provider ?? providerFromEvents(mergedEvents),
        showThinking,
      ),
    [agent?.provider, mergedEvents, provider, sessionId, showThinking],
  );
  const chatRows = useMemo<ChatTranscriptRowModel[]>(
    () => withTurnChangeSummaries(derivePresentedChatRows(sortTranscriptEvents(displayEvents).filter(shouldShowChatEvent))),
    [displayEvents],
  );
  const hiddenOlderRowCount = Math.max(0, chatRows.length - visibleRowLimit);
  const visibleChatRows = useMemo(() => chatRows.slice(hiddenOlderRowCount), [chatRows, hiddenOlderRowCount]);
  const latestVisibleRowKey = visibleChatRows.length > 0 ? chatTranscriptRowKey(visibleChatRows[visibleChatRows.length - 1]) : "";
  const hasActionRequired = mergedEvents.some((event) => event.status === "action_required");
  const liveApprovalId = useMemo(() => liveApprovalEventId(sortTranscriptEvents(mergedEvents)), [mergedEvents]);
  const disabledReason = inputDisabledReason(activeStatus, isSubmitting);
  const openChangedFile = useMemo(() => {
    const workspace = workspacePath?.trim();
    if (!workbenchNavigation || !workspace) return undefined;
    return (path: string) => {
      const absolute = /^([A-Za-z]:[\\/]|\/|\\\\)/.test(path)
        ? path
        : `${workspace.replace(/[\\/]+$/g, "")}/${path.replace(/^[\\/]+/g, "")}`;
      try {
        const surfaceId = workbenchNavigation.open({
          surface_type: "files",
          resource_key: fileResourceKey(absolute),
        });
        workbenchNavigation.pin_transient(surfaceId);
      } catch (reason) {
        console.warn("Failed to open changed file from chat:", reason);
      }
    };
  }, [workbenchNavigation, workspacePath]);
  const markdownLinkHandling = useMemo<ChatMarkdownLinkHandling>(() => ({
    getBasePath: () => workspacePath?.trim() || null,
    getExternalEditor: () => ({
      external_editor: externalEditor,
      external_editor_custom_executable: externalEditorCustomExecutable.trim() || null,
    }),
    onOpenError: (message) => console.warn(message),
  }), [externalEditor, externalEditorCustomExecutable, workspacePath]);

  useEffect(() => {
    stickToLatestRef.current = true;
    prependScrollSnapshotRef.current = null;
    setVisibleRowLimit(CHAT_INITIAL_ROW_LIMIT);
    setAwaitingResponse(null);
    setAttachments([]);
  }, [sessionId]);

  useLayoutEffect(() => {
    const scrollRegion = transcriptScrollRef.current;
    if (!scrollRegion || loadState !== "ready") return;

    const prependSnapshot = prependScrollSnapshotRef.current;
    if (prependSnapshot) {
      scrollRegion.scrollTop = scrollRegion.scrollHeight - prependSnapshot.scrollHeight + prependSnapshot.scrollTop;
      prependScrollSnapshotRef.current = null;
      stickToLatestRef.current = isNearTranscriptBottom(scrollRegion);
      return;
    }

    if (stickToLatestRef.current) {
      scrollRegion.scrollTop = scrollRegion.scrollHeight;
      stickToLatestRef.current = true;
    }
  }, [hiddenOlderRowCount, latestVisibleRowKey, loadState, visibleChatRows.length]);

  const submitPrompt = async (
    promptValue: string,
    clearDraft: boolean,
    selectedAttachments: readonly ChatAttachment[] = [],
  ) => {
    const prompt = promptValue.trim();
    if ((!prompt && selectedAttachments.length === 0) || disabledReason) return;

    const providerName = agent?.provider ?? provider ?? providerFromEvents(events);
    const submittedPrompt = promptWithChatAttachments(prompt, selectedAttachments);

    stickToLatestRef.current = true;
    setIsSubmitting(true);
    setSubmitError(null);
    try {
      await stageChatImageAttachments(sessionId, providerName, selectedAttachments);
      await submitInputToAgent(sessionId, submittedPrompt);
      if (clearDraft) setActiveDraft("");
      if (selectedAttachments.length > 0) setAttachments([]);
      setPendingMessages((pending) => [
        ...pending,
        createPendingUserMessage(
          sessionId,
          providerName,
          submittedPrompt,
          maxSequence(events),
          matchingUserMessageCount(events, submittedPrompt),
        ),
      ]);
      setAwaitingResponse({
        id: `awaiting-response-${sessionId}-${Date.now()}`,
        response_count_after: responseEventCount(events),
      });
      setReloadKey((key) => key + 1);
    } catch (reason) {
      setSubmitError(errorMessage(reason));
    } finally {
      setIsSubmitting(false);
    }
  };

  const handleSubmit = () => {
    void submitPrompt(activeDraft, true, attachments);
  };

  const handleApprovalSubmit = (response: string) => {
    void submitPrompt(response, false);
  };

  const handleTranscriptScroll = () => {
    const scrollRegion = transcriptScrollRef.current;
    if (!scrollRegion || prependScrollSnapshotRef.current) return;
    stickToLatestRef.current = isNearTranscriptBottom(scrollRegion);
  };

  const handleLoadOlderRows = () => {
    const scrollRegion = transcriptScrollRef.current;
    if (scrollRegion) {
      prependScrollSnapshotRef.current = {
        scrollHeight: scrollRegion.scrollHeight,
        scrollTop: scrollRegion.scrollTop,
      };
      stickToLatestRef.current = false;
    }
    setVisibleRowLimit((limit) => limit + CHAT_ROW_PAGE_SIZE);
  };

  return (
    <section
      aria-label={`Chat transcript for ${agent?.session_name ?? sessionId}`}
      className={`agent-chat-view flex h-full min-h-0 flex-col bg-wardian-bg text-primary ${isMaximized ? "text-[14px]" : "text-[13px]"} ${className}`}
      data-theme-mode={theme}
      data-testid="agent-chat-view"
    >
      <div
        className="min-h-0 flex-1 overflow-auto px-3 py-3"
        data-testid="agent-chat-scroll-region"
        onScroll={handleTranscriptScroll}
        ref={transcriptScrollRef}
      >
        {loadState === "loading" ? <LoadingState /> : null}
        {loadState === "error" ? <ErrorState error={error} onRetry={() => setReloadKey((key) => key + 1)} /> : null}
        {loadState === "ready" && chatRows.length === 0 ? <EmptyState /> : null}
        {loadState === "ready" && chatRows.length > 0 ? (
          <ol className="space-y-2" data-testid="agent-chat-transcript">
            {hiddenOlderRowCount > 0 ? (
              <li>
                <button
                  type="button"
                  className="w-full rounded border border-wardian-light bg-[var(--color-wardian-card-bg-muted)] px-3 py-2 text-[12px] font-semibold leading-5 text-muted-neutral hover:text-primary"
                  onClick={handleLoadOlderRows}
                >
                  Load {Math.min(CHAT_ROW_PAGE_SIZE, hiddenOlderRowCount)} earlier transcript rows
                </button>
              </li>
            ) : null}
            {visibleChatRows.map((row) => (
              <li key={chatTranscriptRowKey(row)}>
                <ChatTranscriptRow
                  agentIsWorking={showThinking}
                  isSubmitting={isSubmitting}
                  linkHandling={markdownLinkHandling}
                  onApprovalSubmit={handleApprovalSubmit}
                  liveApprovalId={liveApprovalId}
                  onOpenFile={openChangedFile}
                  row={row}
                />
              </li>
            ))}
          </ol>
        ) : null}
      </div>
      <ChatComposer
        agent={agent}
        autoFocus={autoFocusComposer}
        disabledReason={disabledReason}
        draft={activeDraft}
        hasActionRequired={hasActionRequired}
        isSubmitting={isSubmitting}
        attachments={attachments}
        onAutoFocused={onComposerAutoFocused}
        onAttachmentsChange={setAttachments}
        onChange={setActiveDraft}
        onSubmit={handleSubmit}
        sessionId={sessionId}
        submitError={submitError}
      />
    </section>
  );
}

function isNearTranscriptBottom(scrollRegion: HTMLElement): boolean {
  return scrollRegion.scrollHeight - scrollRegion.scrollTop - scrollRegion.clientHeight <= CHAT_SCROLL_BOTTOM_THRESHOLD_PX;
}


function ChatComposer({
  agent,
  attachments,
  autoFocus,
  disabledReason,
  draft,
  hasActionRequired,
  isSubmitting,
  onAutoFocused,
  onAttachmentsChange,
  onChange,
  onSubmit,
  sessionId,
  submitError,
}: {
  agent?: Pick<AgentConfig, "session_name" | "agent_class" | "provider" | "model" | "provider_config">;
  attachments: readonly ChatAttachment[];
  autoFocus: boolean;
  disabledReason: string | null;
  draft: string;
  hasActionRequired: boolean;
  isSubmitting: boolean;
  onAutoFocused?: () => void;
  onAttachmentsChange: (attachments: ChatAttachment[]) => void;
  onChange: (value: string) => void;
  onSubmit: () => void;
  sessionId: string;
  submitError: string | null;
}) {
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const autoFocusConsumedRef = useRef(false);
  const placeholder = disabledReason ?? (hasActionRequired ? "Respond to action required..." : "Message agent...");
  const canSubmit = (draft.trim().length > 0 || attachments.length > 0) && !disabledReason;

  const chooseAttachments = async () => {
    try {
      const selected = await open({
        directory: false,
        multiple: true,
        title: "Attach files to agent",
      });
      const paths = typeof selected === "string" ? [selected] : selected ?? [];
      if (paths.length === 0) return;

      const knownPaths = new Set(attachments.map((attachment) => attachment.path.toLocaleLowerCase()));
      const added = paths
        .filter((path) => !knownPaths.has(path.toLocaleLowerCase()))
        .map((path) => ({ name: fileNameFromPath(path), path }));
      if (added.length > 0) onAttachmentsChange([...attachments, ...added]);
    } catch (error) {
      console.warn("Failed to choose chat attachments:", error);
    }
  };

  useEffect(() => {
    if (!autoFocus) {
      autoFocusConsumedRef.current = false;
      return;
    }
    if (!disabledReason && !autoFocusConsumedRef.current) {
      textareaRef.current?.focus();
      autoFocusConsumedRef.current = true;
      onAutoFocused?.();
    }
  }, [autoFocus, disabledReason, onAutoFocused]);

  return (
    <form
      className="border-t border-wardian-light bg-[var(--color-wardian-card)] px-3 py-2"
      onSubmit={(event) => {
        event.preventDefault();
        onSubmit();
      }}
    >
      <ChatModelSelection agent={agent} sessionId={sessionId} />
      {attachments.length > 0 ? (
        <div className="mb-2 flex flex-wrap gap-1.5" aria-label="Attached files">
          {attachments.map((attachment) => (
            <span
              className="inline-flex max-w-full items-center gap-1 rounded border border-wardian-light bg-[var(--color-wardian-card-bg-muted)] py-1 pl-2 pr-1 text-[11px] text-primary"
              key={attachment.path}
              title={attachment.path}
            >
              <FileText className="h-3 w-3 shrink-0 text-muted-neutral" aria-hidden="true" />
              <span className="max-w-[20ch] truncate">{attachment.name}</span>
              <button
                aria-label={`Remove ${attachment.name}`}
                className="rounded p-0.5 text-muted-neutral hover:bg-[var(--color-wardian-card)] hover:text-primary"
                disabled={Boolean(disabledReason) || isSubmitting}
                onClick={() => onAttachmentsChange(attachments.filter((item) => item.path !== attachment.path))}
                type="button"
              >
                <X className="h-3 w-3" aria-hidden="true" />
              </button>
            </span>
          ))}
        </div>
      ) : null}
      <div className="flex items-end gap-2">
        <button
          aria-label="Attach files"
          className="inline-flex h-9 w-9 shrink-0 items-center justify-center rounded border border-wardian-light bg-[var(--color-wardian-card-bg-muted)] text-muted-neutral transition-colors hover:border-[var(--color-wardian-accent)] hover:text-primary disabled:cursor-not-allowed disabled:opacity-60"
          disabled={Boolean(disabledReason) || isSubmitting}
          onClick={() => void chooseAttachments()}
          title="Attach files"
          type="button"
        >
          <FilePlus2 className="h-4 w-4" aria-hidden="true" />
        </button>
        <textarea
          aria-label="Message agent"
          className="max-h-28 min-h-9 flex-1 resize-none rounded border border-wardian-light bg-[var(--color-wardian-input-bg)] px-3 py-2 text-[13px] leading-5 text-primary outline-none transition-colors placeholder:text-muted-neutral focus:border-[var(--color-wardian-accent)] disabled:cursor-not-allowed disabled:opacity-70"
          disabled={Boolean(disabledReason)}
          onChange={(event) => onChange(event.target.value)}
          onKeyDown={(event) => {
            if (shouldSubmitComposerKey(event)) {
              event.preventDefault();
              event.stopPropagation();
              if (canSubmit) onSubmit();
            }
          }}
          placeholder={placeholder}
          ref={textareaRef}
          rows={1}
          value={draft}
        />
        <button
          aria-label={isSubmitting ? "Sending message" : "Send message"}
          className="inline-flex h-9 w-9 shrink-0 items-center justify-center rounded border border-[var(--color-wardian-accent)] bg-[color-mix(in_srgb,var(--color-wardian-accent),transparent_86%)] text-[var(--color-wardian-accent)] transition-colors hover:bg-[color-mix(in_srgb,var(--color-wardian-accent),transparent_78%)] disabled:cursor-not-allowed disabled:border-wardian-light disabled:bg-[var(--color-wardian-card-bg-muted)] disabled:text-muted-neutral"
          disabled={!canSubmit}
          type="submit"
        >
          {isSubmitting ? (
            <Loader2 className="h-4 w-4 animate-spin" aria-hidden="true" />
          ) : (
            <SendHorizontal className="h-4 w-4" aria-hidden="true" />
          )}
        </button>
      </div>
      {submitError ? (
        <div className="mt-1 text-[11px] leading-4 text-[var(--color-wardian-error)]" role="alert">
          {submitError}
        </div>
      ) : null}
    </form>
  );
}

function ChatModelSelection({
  agent,
  sessionId,
}: {
  agent?: Pick<AgentConfig, "session_name" | "agent_class" | "provider" | "model" | "provider_config">;
  sessionId: string;
}) {
  const provider = agent?.provider;
  const [selection, setSelection] = useState<ModelSelection>(() => ({
    model: agent?.model,
    reasoning_effort: reasoningEffortForConfig(agent ?? {}),
  }));
  const [saveError, setSaveError] = useState<string | null>(null);
  const [isSaving, setIsSaving] = useState(false);

  useEffect(() => {
    setSelection({
      model: agent?.model,
      reasoning_effort: reasoningEffortForConfig(agent ?? {}),
    });
    setSaveError(null);
  }, [agent?.model, agent?.provider_config, sessionId]);

  if (!provider?.trim()) return null;

  const saveSelection = async (nextSelection: ModelSelection) => {
    const previousSelection = selection;
    setSelection(nextSelection);
    setSaveError(null);
    setIsSaving(true);
    try {
      const saved = await invoke<AgentConfig>("update_agent_model_selection", {
        sessionId,
        model: nextSelection.model ?? null,
        reasoningEffort: nextSelection.reasoning_effort ?? null,
      });
      setSelection({
        model: saved.model,
        reasoning_effort: reasoningEffortForConfig(saved),
      });
    } catch (reason) {
      setSelection(previousSelection);
      setSaveError(errorMessage(reason));
    } finally {
      setIsSaving(false);
    }
  };

  return (
    <div className="mb-2">
      <ProviderModelSelector
        compact
        idPrefix={`chat-${sessionId}`}
        provider={provider}
        selection={selection}
        onSelectionChange={(nextSelection) => void saveSelection(nextSelection)}
      />
      <div className="mt-1 flex items-center justify-between gap-2 text-[10px] text-muted-neutral">
        <span>Applies when this agent next starts or restarts.</span>
        {isSaving ? <span>Saving…</span> : null}
      </div>
      {saveError ? <p className="mt-1 text-[10px] text-wardian-error" role="alert">{saveError}</p> : null}
    </div>
  );
}

function fileNameFromPath(path: string): string {
  const normalized = path.replace(/\\/g, "/");
  const segments = normalized.split("/").filter(Boolean);
  return segments[segments.length - 1] || path;
}

function shouldSubmitComposerKey(event: KeyboardEvent<HTMLTextAreaElement>): boolean {
  if (event.shiftKey || event.nativeEvent.isComposing) return false;
  return event.key === "Enter" || event.key === "NumpadEnter" || event.code === "Enter" || event.code === "NumpadEnter";
}

function LoadingState() {
  return (
    <div className="flex h-full min-h-[160px] items-center justify-center text-[13px] text-muted-neutral">
      Loading transcript...
    </div>
  );
}

function EmptyState() {
  return (
    <div className="flex h-full min-h-[160px] flex-col items-center justify-center gap-1 text-center">
      <div className="text-[13px] font-semibold text-primary">No chat transcript yet</div>
      <div className="max-w-[32ch] text-[12px] leading-5 text-muted-neutral">
        Messages and agent activity will appear here when the provider exposes normalized events.
      </div>
    </div>
  );
}

function ErrorState({ error, onRetry }: { error: string | null; onRetry: () => void }) {
  return (
    <div className="flex h-full min-h-[160px] flex-col items-center justify-center gap-3 text-center">
      <div>
        <div className="text-[13px] font-semibold text-[var(--color-wardian-error)]">Unable to load transcript</div>
        <div className="mt-1 max-w-[42ch] text-[12px] leading-5 text-muted-neutral">{error ?? "The transcript command failed."}</div>
      </div>
      <button
        type="button"
        className="rounded border border-wardian-light px-3 py-1.5 text-[12px] font-semibold text-primary hover:border-[var(--color-wardian-accent)]"
        onClick={onRetry}
      >
        Retry
      </button>
    </div>
  );
}

function mergePendingMessages(events: AgentChatEvent[], pendingMessages: AgentChatEvent[]): AgentChatEvent[] {
  if (pendingMessages.length === 0) return events;
  const unconfirmed = unconfirmedPendingMessages(events, pendingMessages);
  return [...events, ...unconfirmed.map((message, index) => ({ ...message, sequence: pendingSequence(events, index) }))];
}

function appendThinkingIndicator(
  events: AgentChatEvent[],
  sessionId: string,
  provider: string,
  showThinking: boolean,
): AgentChatEvent[] {
  if (!showThinking) return events;

  const sequence = pendingSequence(events, 0);
  return [
    ...events,
    {
      id: `thinking-${sessionId}`,
      session_id: sessionId,
      provider,
      kind: "status",
      role: null,
      text: "Working...",
      title: "Working...",
      status: "processing",
      turn_id: null,
      source: "chat_ui",
      command: null,
      exit_code: null,
      path: null,
      language: null,
      created_at: null,
      sequence,
      metadata: { chat_thinking_indicator: true },
    },
  ];
}


function clearAwaitingResponseWhenAnswered(
  events: AgentChatEvent[],
  marker: AwaitingResponseMarker | null,
): AwaitingResponseMarker | null {
  if (!marker) return null;
  return responseEventCount(events) > marker.response_count_after ? null : marker;
}

function unconfirmedPendingMessages(events: AgentChatEvent[], pendingMessages: AgentChatEvent[]): AgentChatEvent[] {
  const consumedEventIndexes = new Set<number>();
  const consumedTranscriptMatchesByText = new Map<string, number>();

  return pendingMessages.filter((message) => {
    const pendingText = normalizePromptText(message.text ?? "");
    if (!pendingText) return false;
    const confirmAfterMatchingCount = pendingConfirmAfterMatchingUserCount(message);
    if (confirmAfterMatchingCount !== null) {
      const consumed = consumedTranscriptMatchesByText.get(pendingText) ?? 0;
      const matchingCount = matchingUserMessageCount(events, pendingText);
      if (matchingCount > confirmAfterMatchingCount + consumed) {
        consumedTranscriptMatchesByText.set(pendingText, consumed + 1);
        return false;
      }
      return true;
    }

    const confirmAfterSequence = pendingConfirmAfterSequence(message);
    const matchingIndex = events.findIndex((event, index) => {
      if (consumedEventIndexes.has(index)) return false;
      if (event.kind !== "message" || event.role !== "user") return false;
      const sequence = typeof event.sequence === "number" ? event.sequence : 0;
      return sequence > confirmAfterSequence && normalizePromptText(event.text ?? "") === pendingText;
    });
    if (matchingIndex < 0) return true;
    consumedEventIndexes.add(matchingIndex);
    return false;
  });
}

function pendingSequence(events: AgentChatEvent[], offset: number): number {
  return maxSequence(events) + offset + 1;
}

function pendingConfirmAfterSequence(pendingMessage: AgentChatEvent): number {
  const value = pendingMessage.metadata?.confirm_after_sequence;
  return typeof value === "number" ? value : 0;
}

function pendingConfirmAfterMatchingUserCount(pendingMessage: AgentChatEvent): number | null {
  const value = pendingMessage.metadata?.confirm_after_matching_user_count;
  return typeof value === "number" ? value : null;
}

function createPendingUserMessage(
  sessionId: string,
  provider: string,
  text: string,
  confirmAfterSequence: number,
  confirmAfterMatchingUserCount: number,
): AgentChatEvent {
  const createdAt = new Date().toISOString();
  return {
    id: `pending-user-${sessionId}-${createdAt}`,
    session_id: sessionId,
    provider,
    kind: "message",
    role: "user",
    text,
    title: null,
    status: "succeeded",
    turn_id: null,
    source: "chat_input",
    command: null,
    exit_code: null,
    path: null,
    language: null,
    created_at: createdAt,
    sequence: null,
    metadata: {
      optimistic: true,
      confirm_after_sequence: confirmAfterSequence,
      confirm_after_matching_user_count: confirmAfterMatchingUserCount,
    },
  };
}

function maxSequence(events: AgentChatEvent[]): number {
  return events.reduce((max, event) => (typeof event.sequence === "number" ? Math.max(max, event.sequence) : max), 0);
}

function providerFromEvents(events: AgentChatEvent[]): string {
  return events.find((event) => event.provider)?.provider ?? "unknown";
}

function normalizePromptText(value: string): string {
  return value.replace(/\s+/g, " ").trim();
}

function matchingUserMessageCount(events: AgentChatEvent[], text: string): number {
  const normalized = normalizePromptText(text);
  if (!normalized) return 0;
  return events.filter((event) => event.kind === "message" && event.role === "user" && normalizePromptText(event.text ?? "") === normalized)
    .length;
}

function responseEventCount(events: AgentChatEvent[]): number {
  return events.filter((event) => {
    if (event.kind === "message") return event.role === "assistant" || event.role === "system" || event.role === "tool";
    return event.kind === "tool_call" || event.kind === "tool_result" || event.kind === "approval" || event.kind === "terminal_output" || event.kind === "error";
  }).length;
}

function inputDisabledReason(status: string | null, isSubmitting: boolean): string | null {
  if (isSubmitting) return "Sending...";
  const normalized = (status ?? "").toLowerCase();
  if (!normalized) return null;
  if (normalized.includes("action")) return null;
  if (normalized.includes("off")) return "Agent is off";
  if (normalized.includes("headless")) return "Agent is headless";
  if (normalized.includes("paused")) return "Agent is paused";
  if (normalized.includes("error")) return "Agent is in an error state";
  return null;
}

function errorMessage(reason: unknown): string {
  if (reason instanceof Error) return reason.message;
  if (typeof reason === "string") return reason;
  return "The transcript command failed.";
}
