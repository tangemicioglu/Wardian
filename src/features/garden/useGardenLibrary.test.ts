import { describe, expect, it } from "vitest";
import type { LibraryEntry, LibraryIndex, LibraryIndexFolder } from "../../types";
import { gardenLibraryInputs } from "./useGardenLibrary";

function entry(overrides: Partial<LibraryEntry> & { entry_ref: string }): LibraryEntry {
  return {
    kind: "skill",
    path: overrides.entry_ref.split("/").slice(1).join("/"),
    name: overrides.entry_ref.split("/").pop() ?? "",
    description: "",
    tags: [],
    is_starred: false,
    deployment_count: 0,
    ...overrides,
  } as LibraryEntry;
}

function folder(path: string, children: Array<LibraryIndexFolder | LibraryEntry>): LibraryIndexFolder {
  return { path, name: path.split("/").pop() ?? path, children };
}

function indexOf(
  sections: Partial<LibraryIndex["sections"]>,
  deployments: LibraryIndex["deployments"] = {},
): LibraryIndex {
  return { sections, deployments, orphans: [] } as unknown as LibraryIndex;
}

describe("gardenLibraryInputs", () => {
  it("flattens nested skills, prompts, and classes", () => {
    const index = indexOf({
      skills: {
        stubbed: false,
        tree: folder("", [folder("dev", [entry({ entry_ref: "skills/dev/planner" })])]),
      },
      prompts: {
        stubbed: false,
        tree: folder("", [entry({ entry_ref: "prompts/review.md", kind: "prompt" })]),
      },
      classes: {
        stubbed: false,
        tree: folder("", [entry({ entry_ref: "classes/Architect", kind: "class" })]),
      },
    });
    expect(gardenLibraryInputs(index).map((input) => [input.entryRef, input.kind])).toEqual([
      ["classes/Architect", "class"],
      ["prompts/review.md", "prompt"],
      ["skills/dev/planner", "skill"],
    ]);
  });

  it("excludes workflows so a blueprint cannot enter the map twice", () => {
    // Workflows arrive through useGardenWorkflows keyed by Blueprint.id. Adding
    // them here as well would reintroduce the dual-identity bug: one workflow,
    // two units, two positions.
    const index = indexOf({
      workflows: {
        stubbed: false,
        tree: folder("", [entry({ entry_ref: "workflows/build.md", kind: "workflow" })]),
      },
    });
    expect(gardenLibraryInputs(index)).toEqual([]);
  });

  it("skips stubbed sections and entries that failed to parse", () => {
    const index = indexOf({
      mcps: { stubbed: true, tree: folder("", []) },
      skills: {
        stubbed: false,
        tree: folder("", [
          entry({ entry_ref: "skills/broken", error: "bad frontmatter" }),
          entry({ entry_ref: "skills/good" }),
        ]),
      },
    });
    expect(gardenLibraryInputs(index).map((input) => input.entryRef)).toEqual(["skills/good"]);
  });

  it("attaches deployment targets, preserving the linked flag", () => {
    // `linked: false` means the skill was copied rather than junctioned, so its
    // edits do not sync — weaker evidence of relatedness, and worth surfacing.
    const index = indexOf(
      {
        skills: { stubbed: false, tree: folder("", [entry({ entry_ref: "skills/kicad" })]) },
      },
      {
        "skills/kicad": [
          { target_type: "agent", target_id: "a1", linked: true },
          { target_type: "class", target_id: "Architect", linked: false },
        ],
      },
    );
    expect(gardenLibraryInputs(index)[0].deployments).toEqual([
      { targetType: "agent", targetId: "a1", linked: true },
      { targetType: "class", targetId: "Architect", linked: false },
    ]);
  });

  it("returns an empty list before the index loads", () => {
    expect(gardenLibraryInputs(null)).toEqual([]);
  });

  it("sorts deterministically so the layout sees a stable order", () => {
    const index = indexOf({
      skills: {
        stubbed: false,
        tree: folder("", [
          entry({ entry_ref: "skills/zeta" }),
          entry({ entry_ref: "skills/alpha" }),
        ]),
      },
    });
    expect(gardenLibraryInputs(index).map((input) => input.entryRef)).toEqual([
      "skills/alpha",
      "skills/zeta",
    ]);
  });
});
