import { describe, expect, it } from "vitest";
import type { LibraryEntry, LibraryIndex, LibraryIndexFolder } from "../../types";
import { gardenSkillInputs } from "./useGardenSkills";

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

describe("gardenSkillInputs", () => {
  it("flattens nested skills", () => {
    const index = indexOf({
      skills: {
        stubbed: false,
        tree: folder("", [folder("dev", [entry({ entry_ref: "skills/dev/planner" })])]),
      },
    });
    expect(gardenSkillInputs(index).map((input) => input.entryRef)).toEqual([
      "skills/dev/planner",
    ]);
  });

  it("reads no section but skills", () => {
    // Prompts attach to nothing, a class is an attribute an agent already
    // carries, and automations arrive through useGardenAutomations keyed by
    // Blueprint.id — admitting them here would give one automation two units.
    const index = indexOf({
      prompts: {
        stubbed: false,
        tree: folder("", [entry({ entry_ref: "prompts/review.md", kind: "prompt" })]),
      },
      classes: {
        stubbed: false,
        tree: folder("", [entry({ entry_ref: "classes/Architect", kind: "class" })]),
      },
      automations: {
        stubbed: false,
        tree: folder("", [entry({ entry_ref: "automations/build.md", kind: "automation" })]),
      },
    });
    expect(gardenSkillInputs(index)).toEqual([]);
  });

  it("skips a stubbed section and entries that failed to parse", () => {
    const index = indexOf({
      skills: {
        stubbed: false,
        tree: folder("", [
          entry({ entry_ref: "skills/broken", error: "bad frontmatter" }),
          entry({ entry_ref: "skills/good" }),
        ]),
      },
    });
    expect(gardenSkillInputs(index).map((input) => input.entryRef)).toEqual(["skills/good"]);

    expect(gardenSkillInputs(indexOf({ skills: { stubbed: true, tree: folder("", []) } }))).toEqual(
      [],
    );
  });

  it("attaches deployment targets, preserving the linked flag", () => {
    // `linked: false` means the skill was copied rather than junctioned, so its
    // edits never sync back — which the glyph shows with a dashed stroke.
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
    expect(gardenSkillInputs(index)[0].deployments).toEqual([
      { targetType: "agent", targetId: "a1", linked: true },
      { targetType: "class", targetId: "Architect", linked: false },
    ]);
  });

  it("returns an empty list before the index loads", () => {
    expect(gardenSkillInputs(null)).toEqual([]);
  });

  it("sorts deterministically, which monogram assignment depends on", () => {
    const index = indexOf({
      skills: {
        stubbed: false,
        tree: folder("", [
          entry({ entry_ref: "skills/zeta" }),
          entry({ entry_ref: "skills/alpha" }),
        ]),
      },
    });
    expect(gardenSkillInputs(index).map((input) => input.entryRef)).toEqual([
      "skills/alpha",
      "skills/zeta",
    ]);
  });
});
