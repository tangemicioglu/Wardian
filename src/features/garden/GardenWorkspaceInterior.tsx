import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { GardenEntityRef } from "./garden.types";
import { activityChildren } from "./activityFrontier";
import type { GardenTimeLens } from "./gardenNavigation";
import type { TerrainChangeEntry } from "./useTerrainChanges";
import type { TerrainPaint } from "./terrainPaint";
import { basename } from "./terrain";
import type { DirectoryTreeResult } from "../explorer/FileTree";

interface Props {
  path: string;
  entries: ReadonlyMap<string, TerrainChangeEntry>;
  paint: ReadonlyMap<string, TerrainPaint>;
  lens: GardenTimeLens;
  selectedKey: string | null;
  onSelect: (ref: GardenEntityRef) => void;
  onEnter: (ref: GardenEntityRef) => void;
}

/** Activity ancestry is the default; full-tree browsing is explicit and paged. */
export function GardenWorkspaceInterior({ path, entries, paint, lens, selectedKey, onSelect, onEnter }: Props) {
  const [fullTree, setFullTree] = useState(false);
  const [listing, setListing] = useState<DirectoryTreeResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [page, setPage] = useState(0);
  useEffect(() => {
    if (!fullTree) return;
    let active = true;
    void invoke<DirectoryTreeResult>("get_directory_tree", { path, offset: page }).then((result) => {
      if (active) { setListing(result); setError(null); }
    }).catch((reason: unknown) => { if (active) setError(String(reason)); });
    return () => { active = false; };
  }, [path, page, fullTree]);
  const activity = activityChildren(path, entries, paint, lens);
  const children = fullTree && listing
    ? listing.nodes.map((node) => ({ path: node.path, isDirectory: node.is_dir, count: paint.get(node.path)?.count ?? 0, agents: [...(paint.get(node.path)?.agentIds ?? [])] }))
    : activity;
  return <section aria-label="Workspace activity" className="garden-workspace-interior">
    <div className="garden-interior-heading"><div><h2>Workspace</h2><p className="garden-path">{path}</p></div>
      <label><input type="checkbox" checked={fullTree} onChange={(event) => { setFullTree(event.target.checked); setPage(0); }} /> Show full tree</label>
    </div>
    {error && <p role="alert">Directory unavailable: {error}</p>}
    {fullTree && !listing && !error && <p role="status">Loading folder…</p>}
    {children.length === 0 && <p>No file activity in this lens. Show the full tree to browse workspace contents.</p>}
    <div className="garden-activity-groups">{children.map((group) => {
      const ref: GardenEntityRef = { kind: group.isDirectory ? "workspace" : "path", id: group.path };
      const evidence = paint.get(group.path);
      return <button key={group.path} className="garden-organelle" aria-pressed={selectedKey === `${ref.kind}:${ref.id}`}
        onClick={() => onSelect(ref)} onDoubleClick={() => onEnter(ref)} onKeyDown={(event) => { if (event.key === "Enter") { event.preventDefault(); onEnter(ref); } }}>
        <span className="garden-eyebrow">{group.isDirectory ? "Activity group" : "File"}</span>
        <strong>{basename(group.path)}</strong>
        <span>{group.count} changed {group.count === 1 ? "file" : "files"} · {group.agents.length} collaborators</span>
        {evidence && <span>{evidence.kind} · {evidence.evidence}{evidence.evidence === "inferred" || evidence.recencyKnown === false ? " · recency uncertain" : ""}</span>}
      </button>;
    })}</div>
    {fullTree && listing?.next_offset != null && <button onClick={() => setPage(listing.next_offset ?? 0)}>Next folder page</button>}
  </section>;
}
