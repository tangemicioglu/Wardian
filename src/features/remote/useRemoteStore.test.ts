import { beforeEach, describe, expect, it, vi } from "vitest";
import { DEFAULT_WATCHLIST_PREFS } from "../../layout/watchlist/types";
import type { AgentChatEvent, QueueItem } from "../../types";
import { type RemoteAgentChatPage, RemoteRequestError, remoteClient } from "./remoteClient";
import { useRemoteStore } from "./useRemoteStore";

vi.mock("./remoteClient", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./remoteClient")>();
  return {
    ...actual,
      remoteClient: {
        ...actual.remoteClient,
        loadSession: vi.fn(),
        listAgents: vi.fn(),
        listWorkflows: vi.fn(),
        loadWatchlists: vi.fn(),
        loadQueueItems: vi.fn(),
        loadAgentChatPage: vi.fn(),
        openStatusStream: vi.fn(),
      },
  };
});

const session = {
  csrf_nonce: "csrf-1",
  expires_at: "2026-05-21T08:05:00.000Z",
  absolute_expires_at: "2026-05-21T20:00:00.000Z",
};

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function chatMessage(id: string, text: string, sequence: number): AgentChatEvent {
  return {
    id,
    session_id: "agent-1",
    provider: "codex",
    kind: "message",
    role: "assistant",
    text,
    title: null,
    status: null,
    turn_id: "turn-1",
    source: "provider_log",
    command: null,
    exit_code: null,
    path: null,
    language: null,
    created_at: "2026-05-21T08:00:00.000Z",
    sequence,
    metadata: {},
  };
}

describe("useRemoteStore watchlists", () => {
  beforeEach(() => {
    vi.mocked(remoteClient.loadSession).mockResolvedValue(session);
    vi.mocked(remoteClient.listAgents).mockResolvedValue([]);
    vi.mocked(remoteClient.listWorkflows).mockResolvedValue([]);
    vi.mocked(remoteClient.loadWatchlists).mockResolvedValue({ watchlists: [], teams: [], prefs: null });
    vi.mocked(remoteClient.loadQueueItems).mockResolvedValue([]);
    vi.mocked(remoteClient.loadAgentChatPage).mockReset();
    vi.mocked(remoteClient.loadAgentChatPage).mockResolvedValue({ events: [], has_older: false, next_before: null });
    vi.mocked(remoteClient.openStatusStream).mockResolvedValue({ close: vi.fn() } as unknown as WebSocket);
    localStorage.clear();
    useRemoteStore.getState().disconnectStatusStream();
    useRemoteStore.setState({
      agents: [],
      workflows: [],
      watchlists: [],
      teams: [],
      watchlistPrefs: DEFAULT_WATCHLIST_PREFS,
      activeWatchlistId: "all",
      activeRemoteTab: "watchlist",
      mobileCollapsedTeamIds: [],
      activeAgentId: null,
      activeAgentViewModesById: {},
      chatEvents: [],
      chatLoading: false,
      chatLoadingOlder: false,
      chatHasOlder: false,
      chatNextBefore: null,
      chatError: "",
      status: "loading",
    });
  });

  it("loads and normalizes remote watchlists and team state", async () => {
    localStorage.setItem("wardian.remote.activeWatchlistId", "main");
    vi.mocked(remoteClient.loadWatchlists).mockResolvedValue({
      watchlists: [{ id: "main", name: "Main", entries: [{ type: "team", teamId: "team-1" }] }],
      teams: [{ id: "team-1", name: "Core Team", agentIds: ["agent-2", "agent-1"] }],
      prefs: {
        columns: [],
        sort: null,
        preserve_team_grouping_when_sorted: false,
        collapsed_team_ids: ["team-1"],
      },
    });

    await useRemoteStore.getState().load();

    expect(useRemoteStore.getState().watchlists[0]?.id).toBe("main");
    expect(useRemoteStore.getState().teams[0]?.agentIds).toEqual(["agent-2", "agent-1"]);
    expect(useRemoteStore.getState().activeWatchlistId).toBe("main");
    expect(useRemoteStore.getState().mobileCollapsedTeamIds).toEqual([]);
  });

  it("keeps the newest queue response when overlapping loads resolve out of order", async () => {
    const first = deferred<QueueItem[]>();
    const second = deferred<QueueItem[]>();
    vi.mocked(remoteClient.loadQueueItems)
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise);

    const initialLoad = useRemoteStore.getState().load();
    const refreshLoad = useRemoteStore.getState().load();
    const newest = [{
      id: "newest",
      type: "approval_request" as const,
      timestamp: 2,
      read: false,
      notification_title: "Newest approval",
      summary: "New state",
    }];
    const oldest = [{ ...newest[0], id: "oldest", timestamp: 1, notification_title: "Old approval" }];

    second.resolve(newest);
    await refreshLoad;
    expect(useRemoteStore.getState().remoteQueueItems).toEqual(newest);

    first.resolve(oldest);
    await initialLoad;
    expect(useRemoteStore.getState().remoteQueueItems).toEqual(newest);
  });

  it("scopes collapsed team state to the active remote watchlist", () => {
    useRemoteStore.setState({
      activeWatchlistId: "today",
      watchlists: [
        { id: "today", name: "Today", entries: [{ type: "team", teamId: "team-1" }] },
        { id: "later", name: "Later", entries: [{ type: "team", teamId: "team-1" }] },
      ],
      teams: [{ id: "team-1", name: "Core Team", agentIds: ["agent-1", "agent-2"] }],
      mobileCollapsedTeamIds: [],
    });

    useRemoteStore.getState().toggleMobileTeamCollapsed("team-1");
    expect(useRemoteStore.getState().mobileCollapsedTeamIds).toEqual(["team-1"]);

    useRemoteStore.getState().setActiveWatchlistId("later");
    expect(useRemoteStore.getState().mobileCollapsedTeamIds).toEqual([]);

    useRemoteStore.getState().toggleMobileTeamCollapsed("team-1");
    expect(useRemoteStore.getState().mobileCollapsedTeamIds).toEqual(["team-1"]);

    useRemoteStore.getState().setActiveWatchlistId("today");
    expect(useRemoteStore.getState().mobileCollapsedTeamIds).toEqual(["team-1"]);
  });

  it("preserves each mobile agent detail view mode when switching agents", async () => {
    useRemoteStore.setState({
      agents: [
        {
          session_id: "agent-1",
          session_name: "Alpha",
          agent_class: "Coder",
          provider: "codex",
          workspace: "<absolute-workspace-path>",
          status: "Idle",
          latest_text: null,
        },
        {
          session_id: "agent-2",
          session_name: "Beta",
          agent_class: "Coder",
          provider: "codex",
          workspace: "<absolute-workspace-path>",
          status: "Idle",
          latest_text: null,
        },
      ],
      activeAgentId: null,
      activeAgentViewMode: "terminal",
    });

    await useRemoteStore.getState().openAgent("agent-1");
    await useRemoteStore.getState().setActiveAgentViewMode("chat");
    useRemoteStore.getState().closeAgent();

    await useRemoteStore.getState().openAgent("agent-2");
    expect(useRemoteStore.getState().activeAgentViewMode).toBe("terminal");
    await useRemoteStore.getState().setActiveAgentViewMode("terminal");
    useRemoteStore.getState().closeAgent();

    await useRemoteStore.getState().openAgent("agent-1");

    expect(useRemoteStore.getState().activeAgentViewMode).toBe("chat");
  });

  it("fetches chat when reopening an agent whose remembered mobile view mode is chat", async () => {
    vi.mocked(remoteClient.loadAgentChatPage).mockResolvedValue({
      events: [{
        id: "chat-1",
        session_id: "agent-1",
        provider: "codex",
        kind: "message",
        role: "assistant",
        text: "Restored transcript",
        title: null,
        status: null,
        turn_id: "turn-1",
        source: "provider_log",
        command: null,
        exit_code: null,
        path: null,
        language: null,
        created_at: "2026-05-21T08:00:00.000Z",
        sequence: 1,
        metadata: {},
      }],
      has_older: false,
      next_before: null,
    });
    useRemoteStore.setState({
      agents: [
        {
          session_id: "agent-1",
          session_name: "Alpha",
          agent_class: "Coder",
          provider: "codex",
          workspace: "<absolute-workspace-path>",
          status: "Idle",
          latest_text: null,
        },
      ],
      activeAgentId: null,
      activeAgentViewMode: "terminal",
      activeAgentViewModesById: { "agent-1": "chat" },
      chatEvents: [],
    });

    await useRemoteStore.getState().openAgent("agent-1");

    expect(remoteClient.loadAgentChatPage).toHaveBeenCalledWith("agent-1");
    expect(useRemoteStore.getState().activeAgentViewMode).toBe("chat");
    expect(useRemoteStore.getState().chatEvents).toHaveLength(1);
  });

  it("ignores stale remote chat refresh responses that resolve after a newer transcript", async () => {
    const firstLoad = deferred<RemoteAgentChatPage>();
    const secondLoad = deferred<RemoteAgentChatPage>();
    vi.mocked(remoteClient.loadAgentChatPage)
      .mockReturnValueOnce(firstLoad.promise)
      .mockReturnValueOnce(secondLoad.promise);
    useRemoteStore.setState({
      agents: [
        {
          session_id: "agent-1",
          session_name: "Alpha",
          agent_class: "Coder",
          provider: "codex",
          workspace: "<absolute-workspace-path>",
          status: "Processing",
          latest_text: null,
        },
      ],
      activeAgentId: "agent-1",
      activeAgentViewMode: "chat",
      chatEvents: [],
      chatLoading: false,
      chatError: "",
    });

    const firstRefresh = useRemoteStore.getState().refreshActiveAgentChat();
    const secondRefresh = useRemoteStore.getState().refreshActiveAgentChat();

    secondLoad.resolve({ events: [chatMessage("newer-message", "Newer transcript", 2)], has_older: false, next_before: null });
    await secondRefresh;
    expect(useRemoteStore.getState().chatEvents.map((event) => event.text)).toEqual(["Newer transcript"]);

    firstLoad.resolve({
      events: [chatMessage("older-message-1", "Older duplicate", 1), chatMessage("older-message-2", "Older duplicate", 2)],
      has_older: false,
      next_before: null,
    });
    await firstRefresh;

    expect(useRemoteStore.getState().chatEvents.map((event) => event.text)).toEqual(["Newer transcript"]);
  });

  it("loads older remote chat pages only when requested", async () => {
    vi.mocked(remoteClient.loadAgentChatPage)
      .mockResolvedValueOnce({
        events: [chatMessage("newer-message", "Newest transcript", 85)],
        has_older: true,
        next_before: 45,
      })
      .mockResolvedValueOnce({
        events: [chatMessage("older-message", "Older transcript", 45)],
        has_older: false,
        next_before: null,
      })
      .mockResolvedValueOnce({
        events: [chatMessage("newer-message", "Newest transcript", 85)],
        has_older: true,
        next_before: 45,
      });
    useRemoteStore.setState({
      agents: [
        {
          session_id: "agent-1",
          session_name: "Alpha",
          agent_class: "Coder",
          provider: "codex",
          workspace: "<absolute-workspace-path>",
          status: "Idle",
          latest_text: null,
        },
      ],
      activeAgentId: "agent-1",
      activeAgentViewMode: "chat",
    });

    await useRemoteStore.getState().refreshActiveAgentChat();

    expect(useRemoteStore.getState().chatEvents.map((event) => event.text)).toEqual(["Newest transcript"]);
    expect(useRemoteStore.getState().chatHasOlder).toBe(true);
    expect(remoteClient.loadAgentChatPage).toHaveBeenLastCalledWith("agent-1");

    await useRemoteStore.getState().loadOlderActiveAgentChat();

    expect(remoteClient.loadAgentChatPage).toHaveBeenLastCalledWith("agent-1", 45);
    expect(useRemoteStore.getState().chatEvents.map((event) => event.text)).toEqual(["Older transcript", "Newest transcript"]);
    expect(useRemoteStore.getState().chatHasOlder).toBe(false);

    await useRemoteStore.getState().refreshActiveAgentChat({ background: true });

    expect(useRemoteStore.getState().chatEvents.map((event) => event.text)).toEqual(["Older transcript", "Newest transcript"]);
    expect(useRemoteStore.getState().chatHasOlder).toBe(true);
  });

  it("falls back to all agents when the remote watchlist endpoint is unavailable", async () => {
    vi.mocked(remoteClient.loadWatchlists).mockRejectedValue(new RemoteRequestError("not found", 404));

    await useRemoteStore.getState().load();

    expect(useRemoteStore.getState().watchlists).toEqual([]);
    expect(useRemoteStore.getState().teams).toEqual([]);
    expect(useRemoteStore.getState().activeWatchlistId).toBe("all");
  });
});
