/**
 * What a blueprint says about where its workflow belongs.
 *
 * Blueprints used to reach the layout carrying nothing but an id and a name, so
 * every workflow had one facet and they all piled into the commons — the same
 * metric degeneracy that rules untied library assets off the map. But a
 * blueprint is not actually short of canonical evidence; nobody had read it.
 *
 * Three signals, in descending strength:
 *
 * 1. **Agent references.** A `task` or `decision` node naming an agent is a
 *    binding, and the workflow belongs where that agent is.
 * 2. **Workspace paths.** A `shell` node's `cwd` or a `script` node's `path` is
 *    the directory the workflow actually operates on. Agents carry ancestor
 *    facets for the directories they can reach, so a workflow running in
 *    `D:/Trading/trident` shares a rare path facet with the agents that live
 *    there and lands beside them.
 * 3. **Library folder.** `workflows/trident/...` groups a family of workflows
 *    even when none of them names an agent.
 *
 * Which fields count is read from the node registry rather than guessed from
 * field names: the registry declares each field's `kind`, so `path` and
 * `agent_ref` are facts about the schema and not a list of names that drifts.
 */

import type { Blueprint, BlueprintNode } from "../workflows/builder/blueprintTypes";
import { findNodeType } from "../workflows/builder/registry";

export interface WorkflowContext {
  /** Agent ids bound by `agent_ref` fields. */
  agentIds: string[];
  /** Directories named by `path` fields. */
  workspacePaths: string[];
  /** Section-relative folder of the blueprint file, e.g. `trident`. */
  libraryFolder: string | null;
}

/** Field values of `kind`, flattened across the multi-valued case. */
function fieldValuesOfKind(node: BlueprintNode, kind: string): string[] {
  const type = findNodeType(node.type);
  if (!type || !node.fields) return [];
  const values: string[] = [];
  for (const field of type.fields) {
    if (field.kind !== kind) continue;
    const raw = node.fields[field.id];
    for (const value of Array.isArray(raw) ? raw : [raw]) {
      if (typeof value === "string" && value.trim()) values.push(value.trim());
    }
  }
  return values;
}

/**
 * Section-relative folder of a blueprint path.
 *
 * `workflow_list_blueprints` reports an absolute path, so the folder is taken
 * from the segment after `workflows/`. A blueprint sitting directly in the
 * section root has no folder, which is correct: the root is not a grouping.
 */
export function libraryFolderOf(path: string | undefined): string | null {
  if (!path) return null;
  const segments = path.replace(/\\/g, "/").split("/").filter(Boolean);
  const index = segments.lastIndexOf("workflows");
  if (index < 0) return null;
  const between = segments.slice(index + 1, -1);
  return between.length > 0 ? between.join("/").toLowerCase() : null;
}

/** Read the districting evidence out of a parsed blueprint. */
export function workflowContextOf(blueprint: Blueprint, path?: string): WorkflowContext {
  const agentIds = new Set<string>();
  const workspacePaths = new Set<string>();

  for (const node of blueprint.nodes ?? []) {
    for (const value of fieldValuesOfKind(node, "agent_ref")) agentIds.add(value);
    for (const value of fieldValuesOfKind(node, "path")) workspacePaths.add(value);
  }

  return {
    // Sorted so the facet set — and therefore the layout — does not depend on
    // the order nodes happen to appear in the file.
    agentIds: [...agentIds].sort(),
    workspacePaths: [...workspacePaths].sort(),
    libraryFolder: libraryFolderOf(path),
  };
}
