import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { QueueEventType, QueueItem, QueuePreferences } from "../types";
import { extractQueueContent, extractTerminalQueueContent } from "../utils/statusUtils";
import { WorkflowTelemetryEvent } from "../types/workflow";
import { DEFAULT_QUEUE_PREFERENCES, normalizeQueuePreferences, normalizeQueueSoundVolume } from "../features/queue/queueFilters";
import { dispatchQueueNotification } from "../features/queue/queueNotifications";
import { isClearableLegacyCompletion, providerChoiceAcknowledgementUnresolved } from "../features/queue/queueTriage";

export const QUEUE_MAX_AGE_MS = 7 * 24 * 60 * 60 * 1000; // 7 days - future settings hook-in point
const SUMMARY_MAX_CHARS = 500;
const DEDUP_WINDOW_MS = 1_000;
let persistQueue: Promise<void> = Promise.resolve();
let queueMutationRevision = 0;
let loadItemsInFlight: Promise<void> | null = null;

type WorkflowRunIdentity = {
  workflow_id: string;
  workflow_run_id: string;
};

function readProtected(item: QueueItem) {
  return Boolean(
    item.workflow_approval
      || (item.type === "approval_request" && item.notification_status === "awaiting_reply")
      || providerChoiceAcknowledgementUnresolved(item),
  );
}

interface QueueState {
  items: QueueItem[];
  inboxNotificationsTruncated: boolean;
  inboxNotificationsNextOffset: number | null;
  loadingMoreInboxNotifications: boolean;
  preferences: QueuePreferences;
  _agentBuffers: Record<string, string>;
  _workflowLastOutput: Record<string, string>;
  _readNotificationIds: string[];
  _dismissedWorkflowRuns: string[];

  loadItems: () => Promise<void>;
  loadMoreInboxNotifications: () => Promise<void>;
  loadPreferences: () => Promise<void>;
  resolveApprovalRequest: (item: QueueItem, choice: string) => Promise<void>;
  appendAgentEvent: (sessionId: string, data: Record<string, unknown>) => void;
  appendAgentTerminalOutput: (sessionId: string, data: string, provider?: string) => void;
  hasAgentBufferedContent: (sessionId: string) => boolean;
  flushAgentCompletion: (
    sessionId: string,
    agentName: string,
    summary?: string | null,
    evidenceId?: string,
  ) => void;
  addActionNeeded: (
    sessionId: string,
    agentName: string,
    summary?: string | null,
    evidenceId?: string,
    evidenceSource?: QueueItem["evidence_source"],
  ) => void;
  trackWorkflowNodeOutput: (event: WorkflowTelemetryEvent) => void;
  addWorkflowCompletion: (
    payload: {
      workflow_id: string;
      run_instance_id?: string;
      status: "completed" | "failed";
      error?: string;
      summary?: string;
    },
    workflowName?: string,
  ) => void;
  dismissItem: (id: string) => void;
  recordProviderChoiceSent: (id: string, choice: string) => void;
  markRead: (id: string) => void;
  markAllRead: () => void;
  clearRead: () => void;
  setEventVisible: (eventType: QueueEventType, visible: boolean) => void;
  setDesktopNotification: (eventType: QueueEventType, enabled: boolean) => void;
  setSoundNotification: (eventType: QueueEventType, enabled: boolean) => void;
  setSoundVolume: (volume: number) => void;
}

function workflowRunKeyFromIdentity(identity: WorkflowRunIdentity): string {
  return JSON.stringify([identity.workflow_id, identity.workflow_run_id]);
}

function workflowRunIdentityFromKey(key: string): WorkflowRunIdentity | undefined {
  try {
    const value: unknown = JSON.parse(key);
    if (!Array.isArray(value) || value.length !== 2 || value.some((part) => typeof part !== "string")) return undefined;
    return { workflow_id: value[0], workflow_run_id: value[1] };
  } catch {
    return undefined;
  }
}

function workflowRunKey(item: QueueItem): string | undefined {
  if (item.type !== "workflow_completed" || !item.workflow_id || !item.workflow_run_id) return undefined;
  return workflowRunKeyFromIdentity({ workflow_id: item.workflow_id, workflow_run_id: item.workflow_run_id });
}

function workflowDismissalMarker(key: string): QueueItem | undefined {
  const identity = workflowRunIdentityFromKey(key);
  if (!identity) return undefined;
  return {
    id: `workflow-dismissed:${identity.workflow_id}:${identity.workflow_run_id}`,
    type: "workflow_completed",
    timestamp: Date.now(),
    read: true,
    dismissed: true,
    workflow_id: identity.workflow_id,
    workflow_run_id: identity.workflow_run_id,
  };
}

function persistItems(
  items: QueueItem[],
  readNotificationIds: string[] = [],
  dismissedWorkflowRuns: string[] = [],
) {
  const legacyItems = items.filter(
    (item) => !item.inbox_notification_id && !item.workflow_approval && !item.dismissed,
  );
  const readNotificationAcknowledgements = new Set([
    ...readNotificationIds,
    ...items
      .filter((item) => item.type === "agent_update" && item.inbox_notification_id && item.read)
      .map((item) => item.inbox_notification_id!),
  ]);
  const acknowledgementItems = [...readNotificationAcknowledgements].map((notificationId) => ({
      id: `notification-read:${notificationId}`,
      type: "agent_update" as const,
      timestamp: Date.now(),
      read: true,
    inbox_notification_id: notificationId,
  }));
  const workflowDismissalItems = dismissedWorkflowRuns
    .map(workflowDismissalMarker)
    .filter((item): item is QueueItem => item !== undefined);
  persistQueue = persistQueue
    .catch(() => undefined)
    .then(() => invoke("save_queue_items", {
      items: [...legacyItems, ...acknowledgementItems, ...workflowDismissalItems],
    }).then(() => undefined, () => undefined));
}

function persistPreferences(preferences: QueuePreferences) {
  void invoke("save_queue_preferences", { preferences }).then(() => undefined, () => undefined);
}

interface InboxNotificationDto {
  id: string;
  kind: "update" | "approval";
  sender_session_id: string;
  status: "completed" | "awaiting_reply" | "expired";
  title: string;
  body: string;
  proposed_action?: string;
  risk?: string;
  choices: string[];
  expires_at?: string;
  created_at: string;
  decision?: { choice: string };
}

interface InboxNotificationListResult {
  notifications: InboxNotificationDto[];
  truncated: boolean;
  next_offset?: number | null;
}

async function loadInboxNotificationItems(
  readNotificationIds: Set<string>,
  offset?: number,
): Promise<{ items: QueueItem[]; truncated: boolean; nextOffset: number | null }> {
  try {
    const result = await invoke<InboxNotificationListResult>(
      "list_inbox_notifications",
      offset && offset > 0 ? { offset } : undefined,
    );
    const notifications = result.notifications;
    return {
      items: notifications.map((notification) => ({
        id: `notification:${notification.id}`,
        type: notification.kind === "approval" ? "approval_request" : "agent_update",
        timestamp: Date.parse(notification.created_at) || Date.now(),
        read: notification.kind === "update"
          ? readNotificationIds.has(notification.id)
          : notification.status !== "awaiting_reply",
        agent_session_id: notification.sender_session_id,
        notification_title: notification.title,
        inbox_notification_id: notification.id,
        notification_status: notification.status,
        summary: notification.body,
        proposed_action: notification.proposed_action,
        risk: notification.risk,
        approval_choices: notification.choices,
        approval_decision: notification.decision?.choice,
        expires_at: notification.expires_at,
      })),
      truncated: result.truncated,
      nextOffset: result.next_offset ?? null,
    };
  } catch {
    return { items: [], truncated: false, nextOffset: null };
  }
}

interface WorkflowInboxApprovalDto {
  blueprint_id: string;
  blueprint_path: string;
  run_id: string;
  node: string;
  title: string;
  prompt: string;
  created_at?: string;
}

interface WorkflowInboxTerminalDto {
  workflow_id: string;
  run_instance_id: string;
  workflow_name: string;
  status: "completed" | "failed";
  error?: string;
  summary?: string;
  updated_at?: string;
}

async function loadWorkflowApprovalItems(): Promise<QueueItem[]> {
  try {
    const approvals = await invoke<WorkflowInboxApprovalDto[]>("list_workflow_inbox_approvals");
    return approvals.map((approval) => ({
      id: `workflow-approval:${approval.blueprint_id}:${approval.run_id}:${approval.node}`,
      type: "approval_request",
      timestamp: approval.created_at ? Date.parse(approval.created_at) || Date.now() : Date.now(),
      read: false,
      workflow_id: approval.blueprint_id,
      workflow_run_id: approval.run_id,
      workflow_name: approval.title,
      notification_title: approval.title,
      summary: approval.prompt,
      proposed_action: "Continue this workflow beyond its approval gate",
      risk: "The workflow will execute the next authored steps after approval.",
      approval_choices: ["Approve", "Reject"],
      workflow_approval: {
        blueprint_id: approval.blueprint_id,
        blueprint_path: approval.blueprint_path,
        run_id: approval.run_id,
        node: approval.node,
      },
    }));
  } catch {
    return [];
  }
}

async function loadWorkflowTerminalItems(): Promise<QueueItem[]> {
  try {
    const terminalRuns = await invoke<WorkflowInboxTerminalDto[]>("list_workflow_inbox_terminal_runs");
    return terminalRuns.map((run) => ({
      id: `workflow-completion:${run.workflow_id}:${run.run_instance_id}`,
      type: "workflow_completed",
      timestamp: run.updated_at ? Date.parse(run.updated_at) || Date.now() : Date.now(),
      read: false,
      workflow_id: run.workflow_id,
      workflow_run_id: run.run_instance_id,
      workflow_name: run.workflow_name,
      status: run.status,
      error: run.error,
      summary: run.summary ? boundSummary(run.summary) : undefined,
    }));
  } catch {
    return [];
  }
}

function notifyForItem(item: QueueItem, preferences: QueuePreferences) {
  void dispatchQueueNotification(item, preferences);
}

function boundSummary(text: string): string {
  if (text.length <= SUMMARY_MAX_CHARS) return text;
  const marker = "\n...\n";
  const available = SUMMARY_MAX_CHARS - marker.length;
  const headLength = Math.ceil(available * 0.72);
  const tailLength = available - headLength;
  return `${text.slice(0, headLength)}${marker}${text.slice(-tailLength)}`;
}

function isProviderScopedEvidence(evidenceSource: QueueItem["evidence_source"] | undefined) {
  return evidenceSource === "provider_runtime";
}

function matchesActionNeededEvidence(
  item: QueueItem,
  sessionId: string,
  evidenceId: string,
  evidenceSource: QueueItem["evidence_source"] | undefined,
) {
  if (item.type !== "action_needed") return false;
  if (item.evidence_id !== evidenceId || item.evidence_source !== evidenceSource) return false;
  if (isProviderScopedEvidence(evidenceSource)) return item.agent_session_id === sessionId;
  return true;
}

export const useQueueStore = create<QueueState>((set, get) => ({
  items: [],
  inboxNotificationsTruncated: false,
  inboxNotificationsNextOffset: null,
  loadingMoreInboxNotifications: false,
  preferences: DEFAULT_QUEUE_PREFERENCES,
  _agentBuffers: {},
  _workflowLastOutput: {},
  _readNotificationIds: [],
  _dismissedWorkflowRuns: [],

  loadItems() {
    if (loadItemsInFlight) return loadItemsInFlight;

    const request = (async () => {
      try {
        const loadRevision = queueMutationRevision;
        await persistQueue;
        if (loadRevision !== queueMutationRevision) return;
        const raw = await invoke<QueueItem[]>("load_queue_items");
        const cutoff = Date.now() - QUEUE_MAX_AGE_MS;
        const persistedItems = (Array.isArray(raw) ? raw : []).filter((i) => i.timestamp > cutoff);
        const readNotificationIds = new Set(
          persistedItems
            .filter((item) => item.type === "agent_update" && item.read && item.inbox_notification_id)
            .map((item) => item.inbox_notification_id!),
        );
        const dismissedWorkflowRuns = persistedItems
          .filter((item) => item.dismissed)
          .map(workflowRunKey)
          .filter((key): key is string => key !== undefined);
        const legacyItems = persistedItems.filter(
          (item) => !item.inbox_notification_id && !item.workflow_approval && !item.dismissed,
        );
        const [notificationResult, workflowApprovals, workflowTerminals] = await Promise.all([
          loadInboxNotificationItems(readNotificationIds),
          loadWorkflowApprovalItems(),
          loadWorkflowTerminalItems(),
        ]);
        const persistedWorkflowRuns = new Set(
          persistedItems
            .filter((item) => !item.inbox_notification_id && !item.workflow_approval)
            .map(workflowRunKey)
            .filter((key): key is string => key !== undefined),
        );
        const reconciledTerminals = workflowTerminals.filter((item) => {
          const key = workflowRunKey(item);
          return !key || !persistedWorkflowRuns.has(key);
        });
        const items = [...notificationResult.items, ...workflowApprovals, ...reconciledTerminals, ...legacyItems]
          .sort((left, right) => right.timestamp - left.timestamp);
        if (loadRevision !== queueMutationRevision) return;
        set({
          items,
          inboxNotificationsTruncated: notificationResult.truncated,
          inboxNotificationsNextOffset: notificationResult.nextOffset,
          _readNotificationIds: [...readNotificationIds],
          _dismissedWorkflowRuns: [...new Set(dismissedWorkflowRuns)],
        });
        if (reconciledTerminals.length > 0) {
          persistItems(items, [...readNotificationIds], dismissedWorkflowRuns);
        }
      } catch {
        // First run or unavailable: leave items empty.
      }
    })();
    loadItemsInFlight = request;
    void request.then(
      () => {
        if (loadItemsInFlight === request) loadItemsInFlight = null;
      },
      () => {
        if (loadItemsInFlight === request) loadItemsInFlight = null;
      },
    );
    return request;
  },

  async loadMoreInboxNotifications() {
    const offset = get().inboxNotificationsNextOffset;
    if (offset === null || get().loadingMoreInboxNotifications) return;
    set({ loadingMoreInboxNotifications: true });
    try {
      const readNotificationIds = new Set(get()._readNotificationIds);
      const page = await loadInboxNotificationItems(readNotificationIds, offset);
      set((state) => {
        const existing = new Map(
          state.items
            .filter((item) => item.inbox_notification_id)
            .map((item) => [item.inbox_notification_id!, item]),
        );
        for (const item of page.items) existing.set(item.inbox_notification_id!, item);
        const nonNotifications = state.items.filter((item) => !item.inbox_notification_id);
        return {
          items: [...nonNotifications, ...existing.values()].sort((left, right) => right.timestamp - left.timestamp),
          inboxNotificationsTruncated: page.truncated,
          inboxNotificationsNextOffset: page.nextOffset,
        };
      });
    } finally {
      set({ loadingMoreInboxNotifications: false });
    }
  },

  async loadPreferences() {
    try {
      const raw = await invoke<QueuePreferences>("load_queue_preferences");
      set({ preferences: normalizeQueuePreferences(raw) });
    } catch {
      set({ preferences: DEFAULT_QUEUE_PREFERENCES });
    }
  },

  async resolveApprovalRequest(item, choice) {
    if (item.workflow_approval) {
      await invoke("workflow_approve", {
        blueprintId: item.workflow_approval.blueprint_id,
        runId: item.workflow_approval.run_id,
        blueprintPath: item.workflow_approval.blueprint_path,
        node: item.workflow_approval.node,
        granted: choice === "Approve",
        actor: "user",
        note: null,
      });
    } else if (item.inbox_notification_id) {
      await invoke("resolve_inbox_notification", { notificationId: item.inbox_notification_id, choice });
    }
    await get().loadItems();
  },

  appendAgentEvent(sessionId, data) {
    const { text, isToolCall } = extractQueueContent(data);
    if (isToolCall) {
      set((s) => ({ _agentBuffers: { ...s._agentBuffers, [sessionId]: "" } }));
    } else if (text) {
      set((s) => ({
        _agentBuffers: {
          ...s._agentBuffers,
          [sessionId]: boundSummary((s._agentBuffers[sessionId] ?? "") + text),
        },
      }));
    }
  },

  appendAgentTerminalOutput(sessionId, data, provider) {
    if (provider && provider !== "opencode") return;

    const text = extractTerminalQueueContent(data);
    if (!text) return;
    const boundedText = boundSummary(text);
    set((s) => ({
      _agentBuffers: {
        ...s._agentBuffers,
        [sessionId]: boundedText,
      },
    }));
  },

  hasAgentBufferedContent(sessionId) {
    return (get()._agentBuffers[sessionId] ?? "").trim().length > 0;
  },

  flushAgentCompletion(sessionId, agentName, summaryOverride, evidenceId) {
    const { items } = get();
    const summary = summaryOverride?.trim();
    if (!summary) return;
    const recent = items.find(
      (i) => i.type === "agent_completed"
        && i.agent_session_id === sessionId
        && (
          (evidenceId !== undefined && i.evidence_id === evidenceId)
          || Date.now() - i.timestamp < DEDUP_WINDOW_MS
        ),
    );
    if (recent) return;

    const item: QueueItem = {
      id: crypto.randomUUID(),
      type: "agent_completed",
      timestamp: Date.now(),
      read: false,
      agent_session_id: sessionId,
      agent_name: agentName,
      summary: boundSummary(summary),
      evidence_id: evidenceId,
      evidence_source: evidenceId ? "provider_runtime" : undefined,
    };

    queueMutationRevision += 1;
    set((s) => {
      const next = [item, ...s.items];
      persistItems(next, s._readNotificationIds, s._dismissedWorkflowRuns);
      notifyForItem(item, s.preferences);
      return { items: next, _agentBuffers: { ...s._agentBuffers, [sessionId]: "" } };
    });
  },

  addActionNeeded(sessionId, agentName, summary, evidenceId, evidenceSource) {
    const { items, _agentBuffers } = get();
    const recent = items.find((i) => {
      if (evidenceId) return matchesActionNeededEvidence(i, sessionId, evidenceId, evidenceSource);
      return i.type === "action_needed" && i.agent_session_id === sessionId && Date.now() - i.timestamp < DEDUP_WINDOW_MS;
    });
    if (recent) return;

    const explicitSummary = summary?.trim();
    const bufferedSummary = (_agentBuffers[sessionId] ?? "").trim();
    const isGenericSummary = !explicitSummary || /^action needed$/i.test(explicitSummary);
    const itemSummary = isGenericSummary ? (bufferedSummary || explicitSummary || "Action required") : explicitSummary;
    const item: QueueItem = {
      id: crypto.randomUUID(),
      type: "action_needed",
      timestamp: Date.now(),
      read: false,
      agent_session_id: sessionId,
      agent_name: agentName,
      summary: boundSummary(itemSummary),
      evidence_id: evidenceId,
      evidence_source: evidenceSource,
    };

    queueMutationRevision += 1;
    set((s) => {
      const next = [item, ...s.items];
      persistItems(next, s._readNotificationIds, s._dismissedWorkflowRuns);
      notifyForItem(item, s.preferences);
      return { items: next, _agentBuffers: { ...s._agentBuffers, [sessionId]: "" } };
    });
  },

  trackWorkflowNodeOutput(event) {
    if (event.status !== "completed") return;
    const output = event.output as Record<string, unknown> | undefined;
    const text = typeof output?.text === "string" ? output.text : undefined;
    if (text) {
      set((s) => ({ _workflowLastOutput: { ...s._workflowLastOutput, [event.workflow_id]: text } }));
    }
  },

  addWorkflowCompletion(payload, workflowName) {
    const { workflow_id, run_instance_id, status, error } = payload;
    const existing = get().items.find(
      (item) => item.type === "workflow_completed"
        && item.workflow_id === workflow_id
        && item.workflow_run_id === run_instance_id,
    );
    if (existing) return;
    const workflowKey = run_instance_id
      ? workflowRunKeyFromIdentity({ workflow_id, workflow_run_id: run_instance_id })
      : undefined;
    if (workflowKey && get()._dismissedWorkflowRuns.includes(workflowKey)) return;
    const trackedOutput = get()._workflowLastOutput[workflow_id];
    const summary = payload.summary?.trim() || trackedOutput?.trim();
    const item: QueueItem = {
      id: crypto.randomUUID(),
      type: "workflow_completed",
      timestamp: Date.now(),
      read: false,
      workflow_id,
      workflow_run_id: run_instance_id,
      workflow_name: workflowName ?? workflow_id,
      status,
      error,
      summary: summary ? boundSummary(summary) : undefined,
    };

    queueMutationRevision += 1;
    set((s) => {
      const next = [
        item,
        ...s.items.filter((existingItem) => !(
          existingItem.workflow_approval
          && existingItem.workflow_approval.blueprint_id === workflow_id
          && existingItem.workflow_approval.run_id === run_instance_id
        )),
      ];
      persistItems(next, s._readNotificationIds, s._dismissedWorkflowRuns);
      notifyForItem(item, s.preferences);
      return {
        items: next,
        _workflowLastOutput: { ...s._workflowLastOutput, [workflow_id]: "" },
      };
    });
  },

  dismissItem(id) {
    set((s) => {
      const item = s.items.find((candidate) => candidate.id === id);
      if (item && readProtected(item)) return {};
      queueMutationRevision += 1;
      const workflowKey = item ? workflowRunKey(item) : undefined;
      const dismissedWorkflowRuns = workflowKey
        ? [...new Set([...s._dismissedWorkflowRuns, workflowKey])]
        : s._dismissedWorkflowRuns;
      const next = s.items.filter((i) => i.id !== id);
      persistItems(next, s._readNotificationIds, dismissedWorkflowRuns);
      return { items: next, _dismissedWorkflowRuns: dismissedWorkflowRuns };
    });
  },

  recordProviderChoiceSent(id, choice) {
    queueMutationRevision += 1;
    set((s) => {
      const next = s.items.map((item) => (
        item.id === id ? { ...item, provider_choice_sent: choice } : item
      ));
      persistItems(next, s._readNotificationIds, s._dismissedWorkflowRuns);
      return { items: next };
    });
  },

  markRead(id) {
    queueMutationRevision += 1;
    set((s) => {
      const next = s.items.map((i) => (i.id === id ? { ...i, read: true } : i));
      const notificationId = s.items.find((item) => item.id === id)?.inbox_notification_id;
      const readNotificationIds = notificationId
        ? [...new Set([...s._readNotificationIds, notificationId])]
        : s._readNotificationIds;
      persistItems(next, readNotificationIds, s._dismissedWorkflowRuns);
      return { items: next, _readNotificationIds: readNotificationIds };
    });
  },

  markAllRead() {
    queueMutationRevision += 1;
    set((s) => {
      const next = s.items.map((i) => (readProtected(i) ? i : { ...i, read: true }));
      const readNotificationIds = [...new Set([
        ...s._readNotificationIds,
        ...next
          .filter((item) => item.type === "agent_update" && item.inbox_notification_id && item.read)
          .map((item) => item.inbox_notification_id!),
      ])];
      persistItems(next, readNotificationIds, s._dismissedWorkflowRuns);
      return { items: next, _readNotificationIds: readNotificationIds };
    });
  },

  clearRead() {
    queueMutationRevision += 1;
    set((s) => {
      const dismissedWorkflowRuns = new Set(s._dismissedWorkflowRuns);
      const next = s.items.filter((item) => {
        if (!(item.read && isClearableLegacyCompletion(item))) return true;
        const workflowKey = workflowRunKey(item);
        if (workflowKey) dismissedWorkflowRuns.add(workflowKey);
        return false;
      });
      const dismissed = [...dismissedWorkflowRuns];
      persistItems(next, s._readNotificationIds, dismissed);
      return { items: next, _dismissedWorkflowRuns: dismissed };
    });
  },

  setEventVisible(eventType, visible) {
    set((s) => {
      const preferences = {
        ...s.preferences,
        visible_event_types: { ...s.preferences.visible_event_types, [eventType]: visible },
      };
      persistPreferences(preferences);
      return { preferences };
    });
  },

  setDesktopNotification(eventType, enabled) {
    set((s) => {
      const preferences = {
        ...s.preferences,
        desktop_notifications: { ...s.preferences.desktop_notifications, [eventType]: enabled },
      };
      persistPreferences(preferences);
      return { preferences };
    });
  },

  setSoundNotification(eventType, enabled) {
    set((s) => {
      const preferences = {
        ...s.preferences,
        sound_notifications: { ...s.preferences.sound_notifications, [eventType]: enabled },
      };
      persistPreferences(preferences);
      return { preferences };
    });
  },

  setSoundVolume(volume) {
    set((s) => {
      const preferences = {
        ...s.preferences,
        sound_volume: normalizeQueueSoundVolume(volume),
      };
      persistPreferences(preferences);
      return { preferences };
    });
  },
}));
