import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { AgentConfig } from "../../types";

/** Wire models mirror wardian_core::memory; scope belongs to each record. */
export interface GardenMemorySource {
  source_type: string;
  locator?: string;
  source_hash?: string;
  primary: boolean;
}

export interface GardenMemoryRecord {
  revision_id: string;
  memory_id: string;
  revision: number;
  agent_id: string;
  workspace: string | null;
  kind: "stable" | "current";
  text: string;
  evidence_excerpt: string;
  evidence_hash: string;
  status: "active" | "superseded" | "removed";
  supersedes_revision_id: string | null;
  replaced_by_revision_id: string | null;
  created_at: string;
  updated_at: string;
  last_verified_at: string;
  idempotency_key: string | null;
  sources: GardenMemorySource[];
}

/** Canonical archive index, not the live terminal buffer or an execution queue. */
export interface GardenConversationEntry {
  schema: number;
  conversation_id: string;
  agent_id: string;
  agent_name: string;
  agent_class: string;
  workspace: string;
  provider: string;
  provider_session_ids: string[];
  started_at: string;
  ended_at: string | null;
  status: "open" | "closed" | "interrupted";
  boundary_reason: "spawn" | "provider_source_changed" | "clear" | "worktree_switch" | "logging_enabled" | "shutdown";
  first_prompt_excerpt: string | null;
  last_record_excerpt: string | null;
  record_count: number;
  turn_count: number;
  has_turns: boolean;
  lifecycle_only: boolean;
  artifact_count: number;
  path: string;
}

export interface GardenContentState<T> {
  data: T | null;
  loading: boolean;
  error: string | null;
  /** A previous successful snapshot is shown during refresh or after its failure. */
  stale: boolean;
}

/** Fetch a canonical record for the parent-owned Record plane. */
export function readGardenMemory(memoryId: string): Promise<GardenMemoryRecord> {
  return invoke("memory_get", { memoryId });
}

/** Fetch revisions only when a Record plane asks for history. */
export function readGardenMemoryHistory(memoryId: string): Promise<GardenMemoryRecord[]> {
  return invoke("memory_history", { memoryId });
}

function emptyState<T>(): GardenContentState<T> {
  return { data: null, loading: true, error: null, stale: false };
}

function useCanonicalRead<T>(key: string, read: () => Promise<T>, revision: number): GardenContentState<T> {
  const [snapshot, setSnapshot] = useState<{ key: string; value: GardenContentState<T> }>(() => ({ key, value: emptyState<T>() }));
  useEffect(() => {
    let active = true;
    let pending = false;
    const refresh = async () => {
      if (pending) return;
      pending = true;
      setSnapshot((previous) => ({ key, value: {
        ...(previous.key === key ? previous.value : emptyState<T>()),
        loading: true,
        error: null,
        stale: previous.key === key && previous.value.data !== null,
      } }));
      try {
        const data = await read();
        if (active) setSnapshot({ key, value: { data, loading: false, error: null, stale: false } });
      } catch (error) {
        if (active) setSnapshot((previous) => ({ key, value: {
          ...previous.value,
          loading: false,
          error: String(error),
          stale: previous.value.data !== null,
        } }));
      } finally {
        pending = false;
      }
    };
    void refresh();
    // These commands have no frontend invalidation subscription. Refresh only
    // while the cutaway is mounted; each region settles independently.
    const timer = window.setInterval(() => { void refresh(); }, 30_000);
    return () => { active = false; window.clearInterval(timer); };
  }, [key, read, revision]);
  // Never flash the previous agent's private contents before effects run.
  return snapshot.key === key ? snapshot.value : emptyState<T>();
}

/** Lazy cutaway contents. Does not change Library, Inbox, or Workbench selection. */
export function useGardenAgentContents(agent: AgentConfig) {
  const agentId = agent.session_id;
  const workspace = agent.git_worktree_folder || agent.folder;
  const [revision, setRevision] = useState(0);
  const readMemories = useCallback(() => invoke<GardenMemoryRecord[]>("memory_list", {
    agentId, workspace: workspace || null,
  }), [agentId, workspace]);
  const readConversations = useCallback(async () => {
    const result = await invoke<{ schema: number; conversations: GardenConversationEntry[] }>("list_conversations", {
      agent: agentId, scopeAll: false,
    });
    return result.conversations.filter((entry) => entry.agent_id === agentId)
      .sort((left, right) => right.started_at.localeCompare(left.started_at));
  }, [agentId]);
  const memories = useCanonicalRead(JSON.stringify([agentId, workspace]), readMemories, revision);
  const conversations = useCanonicalRead(agentId, readConversations, revision);
  const refresh = useCallback(() => setRevision((value) => value + 1), []);
  return { memories, conversations, refresh };
}
