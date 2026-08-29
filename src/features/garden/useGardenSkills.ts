import { useEffect, useMemo } from "react";
import type { LibraryEntry, LibraryIndex, LibraryIndexFolder } from "../../types";
import { isLibraryEntry } from "../../types";
import { useLibraryStore } from "../../store/useLibraryStore";

export interface GardenSkillDeployment {
  /** `user`, `class`, or `agent`; see `get_target_skills_dir`. */
  targetType: string;
  /** Agent id, class directory name, or `global` for the user scope. */
  targetId: string;
  /** False when the skill was copied rather than junctioned; edits do not sync. */
  linked: boolean;
}

export interface GardenSkillInput {
  /** `skills/<rel_path>`, the library's own stable identity. */
  entryRef: string;
  label: string;
  tags: string[];
  deployments: GardenSkillDeployment[];
}

/**
 * Skills are the only library section the Garden reads.
 *
 * The others fail the attribute-versus-lifecycle test that decides what earns a
 * unit (see `skillGlyphs.ts`):
 *
 * - **classes** are an attribute of an agent, and an agent has exactly one, so
 *   the class already shows up as the agent's district fallback rather than as
 *   a second thing to look at.
 * - **prompts** attach to nothing. The library records no relationship for them
 *   at all, so there is no tie to draw and no defensible place to put one.
 * - **automations** enter through `useGardenAutomations` keyed by `Blueprint.id`.
 *   Admitting them here as well would reintroduce the dual-identity bug
 *   `entityRef.ts` exists to prevent.
 * - **mcps** is stubbed in the backend and never has entries.
 *
 * None of them are lost — the Library is the surface built for browsing
 * everything.
 */
const SKILLS_SECTION = "skills";

/** Flatten the library index into Garden skill inputs. Pure, so it is directly testable. */
export function gardenSkillInputs(index: LibraryIndex | null): GardenSkillInput[] {
  if (!index) return [];

  const section = index.sections?.[SKILLS_SECTION];
  if (!section || section.stubbed) return [];

  const inputs: GardenSkillInput[] = [];
  for (const entry of flattenEntries(section.tree)) {
    // An entry that failed to parse has no trustworthy label or tags; drawing
    // it would assert a capability the file cannot back.
    if (entry.error) continue;
    inputs.push({
      entryRef: entry.entry_ref,
      label: entry.name,
      tags: entry.tags ?? [],
      deployments: (index.deployments?.[entry.entry_ref] ?? []).map((target) => ({
        targetType: target.target_type,
        targetId: target.target_id,
        linked: target.linked,
      })),
    });
  }
  // Sorted so monogram assignment and the facet pass see a stable order.
  return inputs.sort((left, right) => left.entryRef.localeCompare(right.entryRef));
}

function flattenEntries(folder: LibraryIndexFolder): LibraryEntry[] {
  const entries: LibraryEntry[] = [];
  const walk = (node: LibraryIndexFolder) => {
    for (const child of node.children) {
      if (isLibraryEntry(child)) entries.push(child);
      else walk(child);
    }
  };
  walk(folder);
  return entries;
}

/**
 * Skills for the Garden.
 *
 * Reads the shared library store rather than invoking directly: the Library view
 * already owns fetching and refreshes on the `library-changed` event, so the
 * Garden gets updates for free and cannot disagree with it about what exists.
 */
export function useGardenSkills(enabled = true): GardenSkillInput[] {
  const index = useLibraryStore((state) => state.index);
  const fetchIndex = useLibraryStore((state) => state.fetchIndex);

  useEffect(() => {
    if (!enabled || index) return;
    void fetchIndex();
  }, [enabled, index, fetchIndex]);

  return useMemo(() => (enabled ? gardenSkillInputs(index) : []), [enabled, index]);
}
