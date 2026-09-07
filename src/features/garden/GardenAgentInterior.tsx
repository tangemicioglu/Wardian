import { useEffect, useId, useState, type ReactNode } from "react";
import type { AgentConfig, QueueItem } from "../../types";
import { useQueueStore } from "../../store/useQueueStore";
import { normalizeAgentConfig } from "../agents/configUtils";
import type { AgentTeam } from "../../layout/watchlist/types";
import type { GardenEntityRef } from "./garden.types";
import type { GardenAutomationInput } from "./gardenProjection";
import type { GardenSkillGlyph } from "./skillGlyphs";
import { normalizeEntityPath } from "./entityRef";
import { agentMonogram } from "./agentMonogram";
import { useGardenAgentContents, type GardenContentState } from "./useGardenAgentContents";
import "./garden-agent-interior.css";

export interface GardenAgentInteriorProps {
  agent: AgentConfig;
  status: string;
  crown: GardenSkillGlyph[];
  agents: AgentConfig[];
  teams: AgentTeam[];
  automations: GardenAutomationInput[];
  selectedKey?: string | null;
  onSelect: (ref: GardenEntityRef) => void;
  onEnter: (ref: GardenEntityRef) => void;
  onOpenAgent: (id: string) => void;
}

function Region({ name, children }: { name: string; children: ReactNode }) {
  const id = useId();
  return <section className={`garden-agent-interior-region garden-agent-interior-${name.toLowerCase().replace(/ /g, "-")}`} aria-labelledby={id}>
    <h3 id={id}>{name}</h3>{children}
  </section>;
}

function ContentNotice({ state, label }: { state: GardenContentState<unknown>; label: string }) {
  return <>
    {state.loading && <p role="status">{state.stale ? `Refreshing ${label}…` : `Loading ${label}…`}</p>}
    {state.error && <p role="status">{label} unavailable: {state.error}</p>}
    {state.stale && <p className="garden-agent-interior-note">Showing the last loaded snapshot.</p>}
  </>;
}

function queueItemStatus(item: QueueItem): string {
  if (item.notification_status === "expired") return "Expired";
  if (item.approval_decision) return `Decision: ${item.approval_decision}`;
  if (item.provider_choice_pending) return "Sending response";
  if (item.provider_choice_sent) return "Response sent";
  if (item.notification_status === "awaiting_reply" || item.automation_approval) return "Awaiting approval";
  if (item.notification_status === "completed") return "Completed";
  if (item.type === "action_needed" || item.type === "approval_request") return "Action required";
  if (item.status === "failed") return "Failed";
  if (item.type === "agent_completed" || item.type === "automation_completed") return "Completed";
  return "Update";
}

/** Inbox attribution uses stable session identity, never a display name or blueprint guess. */
function AgentQueue({ agentId }: { agentId: string }) {
  const items = useQueueStore((state) => state.items);
  const loadItems = useQueueStore((state) => state.loadItems);
  const truncated = useQueueStore((state) => state.inboxNotificationsTruncated);
  const loadMore = useQueueStore((state) => state.loadMoreInboxNotifications);
  const loadingMore = useQueueStore((state) => state.loadingMoreInboxNotifications);
  const [loading, setLoading] = useState(true);
  useEffect(() => {
    let active = true;
    void loadItems().finally(() => { if (active) setLoading(false); });
    return () => { active = false; };
  }, [loadItems]);
  const attributed = items.filter((item) => item.agent_session_id === agentId && !item.dismissed)
    .sort((left, right) => right.timestamp - left.timestamp);
  return <div className="garden-agent-interior-queue">
    <h4>Inbox work and updates</h4>
    {loading && <p role="status">Loading Inbox…</p>}
    {attributed.map((item) => <article key={item.id} className="garden-agent-interior-conversation">
      <strong>{item.notification_title || queueItemStatus(item)}</strong>
      <p>{item.summary || item.proposed_action || item.error || "No summary recorded."}</p>
      <small>{queueItemStatus(item)} · {item.read ? "Read" : "Unread"}</small>
    </article>)}
    {!loading && !attributed.length && <p>No attributable items in the loaded Inbox.</p>}
    {truncated && <button type="button" className="garden-agent-interior-action" disabled={loadingMore} onClick={() => { void loadMore(); }}>
      {loadingMore ? "Loading older Inbox items…" : "Load older Inbox items"}
    </button>}
  </div>;
}

const PERMISSION_FIELDS = {
  permission_mode: "Permission mode", sandbox_mode: "Sandbox", approval_policy: "Approval policy",
  sandbox: "Sandbox enabled", yolo: "Bypass approvals", approval_mode: "Approval mode",
  dangerously_skip_permissions: "Skip permission checks", mode: "Operating mode", auto: "Automatic approvals",
  full_auto: "Full auto", project_trust: "Project trust", policy: "Policy files", admin_policy: "Admin policy files",
  strict_mcp_config: "Strict MCP configuration", offline: "Offline",
};
const TOOL_FIELDS = {
  tools: "Tools", allowed_tools: "Allowed tools", disallowed_tools: "Disallowed tools",
  exclude_tools: "Excluded tools", no_tools: "Disable tools", allowed_mcp_server_names: "Allowed MCP servers",
  extensions: "Extensions", search: "Web search",
};

function ConfigurationFields({ config, fields }: { config: Record<string, unknown>; fields: Record<string, string> }) {
  const rows = Object.entries(fields).flatMap(([key, label]) => {
    const value = config[key];
    if (value === undefined || value === null) return [];
    if (typeof value === "boolean") return [[label, value ? "Yes" : "No"]];
    if (typeof value === "string") return [[label, value || "Not specified"]];
    if (Array.isArray(value) && value.every((item) => typeof item === "string")) return [[label, value.join(", ") || "None explicitly configured"]];
    return [];
  });
  return rows.length ? <dl className="garden-agent-interior-configuration">{rows.map(([label, value]) => <div key={label}><dt>{label}</dt><dd>{value}</dd></div>)}</dl>
    : <p>No explicit overrides; provider defaults apply.</p>;
}

/** Content only: the parent owns the membrane, camera, selection, and navigation. */
export function GardenAgentInterior({ agent, status, crown, agents, teams, automations, selectedKey, onSelect, onEnter, onOpenAgent }: GardenAgentInteriorProps) {
  const contents = useGardenAgentContents(agent);
  const normalizedConfig = normalizeAgentConfig(agent);
  const providerConfig = (normalizedConfig.provider_config ?? {}) as Record<string, unknown>;
  const workspace = agent.git_worktree_folder || agent.folder;
  const workspaceId = normalizeEntityPath(workspace);
  const memberships = teams.filter((team) => team.agentIds.includes(agent.session_id));
  const routines = automations.filter((automation) => automation.agentIds?.includes(agent.session_id));
  const peers = agents.filter((peer) => peer.session_id !== agent.session_id && (
    (workspaceId !== null && normalizeEntityPath(peer.git_worktree_folder || peer.folder) === workspaceId)
    || memberships.some((team) => team.agentIds.includes(peer.session_id))
  ));

  const record = (ref: GardenEntityRef, title: string, detail?: ReactNode) => <div className="garden-agent-interior-record" key={`${ref.kind}:${ref.id}`}>
    <button type="button" className="garden-agent-interior-select" aria-pressed={selectedKey === `${ref.kind}:${ref.id}`} onClick={() => onSelect(ref)} onDoubleClick={(event) => { event.stopPropagation(); onEnter(ref); }}
      onKeyDown={(event) => {
        if (event.key === "Enter") { event.preventDefault(); event.stopPropagation(); onEnter(ref); }
        if (event.key === " ") event.stopPropagation();
      }}>
      <strong>{title}</strong>{detail && <> <span>{detail}</span></>}
    </button>
  </div>;

  return <div className="garden-agent-interior" onClick={(event) => event.stopPropagation()} onDoubleClick={(event) => event.stopPropagation()} onPointerDown={(event) => event.stopPropagation()}>
    <Region name="Identity">
      <div className="garden-agent-interior-sigil" aria-hidden="true">{agentMonogram(agent.session_name)}</div>
      {record({ kind: "identity", id: agent.session_id }, agent.session_name,
        <>{agent.agent_class} · {agent.provider || "Provider unspecified"}{agent.model ? ` · ${agent.model}` : ""}<br />{status}</>)}
      {agent.description && <p>{agent.description}</p>}
      <h4>Configured permissions</h4>
      <ConfigurationFields config={providerConfig} fields={PERMISSION_FIELDS} />
      <p className="garden-agent-interior-note">Saved configuration; runtime application may require a restart.</p>
      <button type="button" className="garden-agent-interior-action" onClick={() => onOpenAgent(agent.session_id)}>Open agent session</button>
    </Region>
    <Region name="Capabilities">
      {crown.length ? crown.map((skill) => record({ kind: "skill", id: skill.entryRef }, skill.label,
        `${skill.provenance === "class" ? "Class-inherited" : skill.provenance === "global" ? "Global" : "Direct"} · ${skill.copied ? "Copied; does not sync" : "Linked"}`))
        : <p>No deployed skills in this projection.</p>}
      <h4>Configured tools</h4>
      <ConfigurationFields config={providerConfig} fields={TOOL_FIELDS} />
      {typeof providerConfig.mcp_config === "string" && providerConfig.mcp_config && <p>MCP configuration supplied.</p>}
    </Region>
    <Region name="Memory">
      <ContentNotice state={contents.memories} label="Memory" />
      {(["stable", "current"] as const).map((kind) => <div key={kind} className="garden-agent-interior-memory-kind">
        <h4>{kind === "stable" ? "Stable" : "Current"}</h4>
        {([false, true] as const).map((workspaceBound) => {
          const records = contents.memories.data?.filter((memory) => memory.kind === kind && (memory.workspace !== null) === workspaceBound) ?? [];
          return records.length > 0 && <div key={String(workspaceBound)} className="garden-agent-interior-scope">
            <h5>{workspaceBound ? "Workspace-bound" : "Agent-wide"}</h5>
            {records.map((memory) => record({ kind: "memory", id: memory.memory_id }, memory.text,
              <>{memory.workspace || "All workspaces"} · Revision {memory.revision}<br />{memory.evidence_excerpt}</>))}
          </div>;
        })}
      </div>)}
      {contents.memories.data?.length === 0 && <p>No active memories in this scope.</p>}
      <button type="button" className="garden-agent-interior-action" onClick={contents.refresh}>Refresh contents</button>
    </Region>
    <Region name="Active work">
      <ContentNotice state={contents.conversations} label="Conversations" />
      {contents.conversations.data?.slice(0, 3).map((conversation) => <div className="garden-agent-interior-conversation" key={conversation.conversation_id}>
        <strong>{conversation.status === "open" ? "Current conversation" : "Recent conversation"}</strong>
        <p>{conversation.last_record_excerpt || conversation.first_prompt_excerpt || "No recorded excerpt."}</p>
        <small>{conversation.status} · {conversation.turn_count} turns · {conversation.artifact_count} artifacts</small>
      </div>)}
      {contents.conversations.data?.length === 0 && <p>No recorded conversations available.</p>}
      {routines.map((routine) => record({ kind: "automation", id: routine.id }, routine.label, routine.runStatus === "none" ? "Assigned routine" : routine.runStatus))}
      <AgentQueue agentId={agent.session_id} />
    </Region>
    <Region name="Ports">
      {workspaceId && record({ kind: "workspace", id: workspaceId }, workspace, "Workspace")}
      {memberships.map((team) => <p key={team.id}>Team · {team.name}</p>)}
      {peers.map((peer) => record({ kind: "agent", id: peer.session_id }, peer.session_name,
        memberships.some((team) => team.agentIds.includes(peer.session_id)) ? "Shared team" : "Shared workspace"))}
      {!workspaceId && !memberships.length && !peers.length && <p>No workspace or team relationships available.</p>}
    </Region>
  </div>;
}
