import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { AgentConfig } from "../../types";
import type { GardenEntityRef } from "./garden.types";
import type { GardenSkillGlyph } from "./skillGlyphs";
import type { TerrainChangeEntry } from "./useTerrainChanges";
import { readGardenMemory, readGardenMemoryHistory } from "./useGardenAgentContents";
import { useFileResource } from "../files/useFileResource";
import { fileResourceClient } from "../files/fileResourceClient";

function useRecordRead<T>(key: string, read: () => Promise<T>) {
  const [result, setResult] = useState<{ key: string; value?: T; error?: string }>({ key });
  const [retry, setRetry] = useState(0);
  useEffect(() => {
    let active = true;
    void read().then((value) => { if (active) setResult({ key, value }); })
      .catch((error: unknown) => { if (active) setResult({ key, error: String(error) }); });
    return () => { active = false; };
    // The canonical key is the request identity; inline readers must not reload on paint.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, retry]);
  return { ...(result.key === key ? result : { key }), retry: () => setRetry((value) => value + 1) };
}

function RecordText({ value, error, retry }: { value?: string; error?: string; retry: () => void }) {
  if (error) return <div role="alert"><p>Record unavailable: {error}</p><button onClick={retry}>Retry</button></div>;
  if (value === undefined) return <p role="status">Loading record…</p>;
  return <pre className="garden-record-text">{value || "This record is empty."}</pre>;
}

function MemoryRecord({ id }: { id: string }) {
  const result = useRecordRead(id, () => Promise.all([
    readGardenMemory(id),
    readGardenMemoryHistory(id),
  ]));
  const memory = result.value?.[0];
  return <>
    <RecordText value={memory?.text} error={result.error} retry={result.retry} />
    {memory && <>
      <dl className="garden-record-facts"><dt>Scope</dt><dd>{memory.workspace ?? "Agent-wide"}</dd><dt>Kind</dt><dd>{memory.kind}</dd><dt>Status</dt><dd>{memory.status}</dd><dt>Revision</dt><dd>{memory.revision}</dd><dt>Last verified</dt><dd>{memory.last_verified_at}</dd></dl>
      <h3>Evidence</h3><blockquote>{memory.evidence_excerpt}</blockquote>
      <h3>Sources</h3>{memory.sources.map((source, index) => <p key={index}>{source.source_type} · {source.locator ?? "No locator recorded"}</p>)}
      <details><summary>Revision history ({result.value?.[1].length ?? 0})</summary>{result.value?.[1].map((revision) => <section key={revision.revision_id}><h4>Revision {revision.revision} · {revision.updated_at}</h4><p>{revision.text}</p><blockquote>{revision.evidence_excerpt}</blockquote></section>)}</details>
    </>}
  </>;
}

function SkillRecord({ id, glyph }: { id: string; glyph?: GardenSkillGlyph }) {
  const result = useRecordRead(id, () => invoke<string>("read_library_item", { section: "skills", path: id.replace(/^skills\//, "") }));
  return <><p>{glyph?.provenance ?? "Library"} deployment · {glyph ? glyph.copied ? "Copied deployment" : "Linked deployment" : "Deployment not loaded"}</p><RecordText {...result} /></>;
}

function FileRecord({ path, change }: { path: string; change?: TerrainChangeEntry }) {
  const resource = useFileResource({ path, agent_id: null, user_file_capability_id: null });
  const snapshot = resource.snapshot;
  const content = useRecordRead(`${path}:${snapshot?.revision ?? "loading"}`, async () => {
    if (!snapshot) return undefined;
    return (await fileResourceClient.readText(snapshot)).text;
  });
  return <>
    <p className="garden-path">{path}</p>
    {change && <dl className="garden-record-facts"><dt>Change</dt><dd>{change.entry.change_kind} · +{change.entry.insertions ?? 0} / −{change.entry.deletions ?? 0}</dd><dt>Evidence</dt><dd>{change.entry.evidence}</dd><dt>Agents</dt><dd>{change.entry.agent_ids.join(", ") || "No attributed agent"}</dd><dt>Baseline</dt><dd>{change.baselineRef ?? "Working tree"}</dd><dt>Turns</dt><dd>{change.entry.turn_indices.join(", ") || "Unknown"}</dd></dl>}
    <RecordText value={content.value} error={resource.error?.message ?? content.error} retry={() => { void resource.retry(); content.retry(); }} />
  </>;
}

export function GardenRecord({ target, agent, glyph, change, onOpenAgent, onOpenSkill, onOpenPath }: {
  target: GardenEntityRef;
  agent?: AgentConfig;
  glyph?: GardenSkillGlyph;
  change?: TerrainChangeEntry;
  onOpenAgent: (id: string) => void;
  onOpenSkill: (id: string) => void;
  onOpenPath: (id: string) => void;
}) {
  return <article className="garden-record" aria-label={`${target.kind} record`}>
    <span className="garden-eyebrow">{target.kind} · Canonical record</span>
    {target.kind === "memory" && <MemoryRecord id={target.id} />}
    {target.kind === "skill" && <><h2>{glyph?.label ?? target.id}</h2><SkillRecord id={target.id} glyph={glyph} /><button onClick={() => onOpenSkill(target.id)}>Open in Library</button></>}
    {target.kind === "path" && <><FileRecord path={target.id} change={change} /><button onClick={() => onOpenPath(target.id)}>Open file</button></>}
    {target.kind === "identity" && (agent ? <><h2>{agent.session_name}</h2><p>{agent.description || "No purpose recorded."}</p><dl className="garden-record-facts"><dt>Class</dt><dd>{agent.agent_class}</dd><dt>Provider</dt><dd>{agent.provider ?? "Default provider"}</dd><dt>Model</dt><dd>{agent.model ?? "Provider default"}</dd><dt>Workspace</dt><dd>{agent.git_worktree_folder ?? agent.folder}</dd><dt>Instructions</dt><dd>{agent.append_system_prompt || "No additional system prompt configured."}</dd></dl><button onClick={() => onOpenAgent(agent.session_id)}>Open agent session</button></> : <p>This agent is no longer in the current roster.</p>)}
  </article>;
}
