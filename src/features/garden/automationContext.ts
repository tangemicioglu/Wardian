/**
 * What a blueprint says about where its automation belongs.
 *
 * Blueprints used to reach the layout carrying nothing but an id and a name, so
 * every automation had one facet and they all piled into the commons — the same
 * metric degeneracy that rules untied library assets off the map. But a
 * blueprint is not actually short of canonical evidence; nobody had read it.
 *
 * Three signals, in descending strength:
 *
 * 1. **Agent references.** A `task` or `decision` node naming an agent is a
 *    binding, and the automation belongs where that agent is.
 * 2. **Workspace paths.** A `shell` node's `cwd` or a `script` node's `path` is
 *    the directory the automation actually operates on. Agents carry ancestor
 *    facets for the directories they can reach, so an automation running in
 *    `D:/Trading/trident` shares a rare path facet with the agents that live
 *    there and lands beside them.
 * 3. **Library folder.** `automations/trident/...` groups a family of automations
 *    even when none of them names an agent.
 *
 * Which fields count is read from the node registry rather than guessed from
 * field names: the registry declares each field's `kind`, so `path` and
 * `agent_ref` are facts about the schema and not a list of names that drifts.
 */

import type { Blueprint, BlueprintNode } from "../automations/builder/blueprintTypes";
import { findNodeType } from "../automations/builder/registry";

export interface AutomationContext {
  /** Concrete agent ids bound by `agent_ref` fields. */
  agentIds: string[];
  /** Role names a blueprint leaves open, e.g. `evolver` from `role:evolver`. */
  roleNames: string[];
  /** Agent classes a blueprint requires, e.g. `Coder` from `class:Coder`. */
  classNames: string[];
  /** Directories named by `path` fields. */
  workspacePaths: string[];
  /** Section-relative folder of the blueprint file, e.g. `trident`. */
  libraryFolder: string | null;
}

/**
 * An `agent_ref` is one of three different things, and they were being read as
 * one.
 *
 * A bare value is an agent id and binds the automation to that agent. `role:name`
 * and `class:name` are *unfilled* — they say what kind of agent the automation
 * needs, and which actual agent runs it is decided elsewhere (at launch, or in
 * the schedule that deploys it). `ephemeral` names a throwaway agent and ties
 * the automation to nobody.
 *
 * Treating all of them as ids is why the Evolver's automations sat in the commons:
 * every task node in them says `role:evolver`, which resolved to an agent id
 * that does not exist, so the automation had no link to anywhere. The role is
 * still worth recording — automations sharing one belong together even when none
 * of them is deployed — but it is not a binding, and only bindings place a
 * automation in an agent's district.
 */
function classifyAgentRef(value: string): { kind: "agent" | "role" | "class"; name: string } | null {
  if (value === "ephemeral") return null;
  if (value.startsWith("role:")) {
    const name = value.slice("role:".length).trim();
    return name ? { kind: "role", name } : null;
  }
  if (value.startsWith("class:")) {
    const name = value.slice("class:".length).trim();
    return name ? { kind: "class", name } : null;
  }
  return { kind: "agent", name: value };
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
 * `automation_list_blueprints` reports an absolute path, so the folder is taken
 * from the segment after `automations/`. A blueprint sitting directly in the
 * section root has no folder, which is correct: the root is not a grouping.
 */
export function libraryFolderOf(path: string | undefined): string | null {
  if (!path) return null;
  const segments = path.replace(/\\/g, "/").split("/").filter(Boolean);
  const index = segments.lastIndexOf("automations");
  if (index < 0) return null;
  const between = segments.slice(index + 1, -1);
  return between.length > 0 ? between.join("/").toLowerCase() : null;
}

/** Read the districting evidence out of a parsed blueprint. */
export function automationContextOf(blueprint: Blueprint, path?: string): AutomationContext {
  const agentIds = new Set<string>();
  const roleNames = new Set<string>();
  const classNames = new Set<string>();
  const workspacePaths = new Set<string>();

  for (const node of blueprint.nodes ?? []) {
    for (const value of fieldValuesOfKind(node, "agent_ref")) {
      const ref = classifyAgentRef(value);
      if (!ref) continue;
      if (ref.kind === "agent") agentIds.add(ref.name);
      else if (ref.kind === "role") roleNames.add(ref.name);
      else classNames.add(ref.name);
    }
    for (const value of fieldValuesOfKind(node, "path")) workspacePaths.add(value);
  }

  return {
    // Sorted so the facet set — and therefore the layout — does not depend on
    // the order nodes happen to appear in the file.
    agentIds: [...agentIds].sort(),
    roleNames: [...roleNames].sort(),
    classNames: [...classNames].sort(),
    workspacePaths: [...workspacePaths].sort(),
    libraryFolder: libraryFolderOf(path),
  };
}
