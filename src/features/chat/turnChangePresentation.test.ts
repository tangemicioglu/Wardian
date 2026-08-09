import { describe, expect, it } from "vitest";

import type { TurnChangeFile } from "./chatTurns";
import {
  TURN_CHANGE_AUTO_EXPAND_FILE_LIMIT,
  changeScope,
  changedFileDirectory,
  changedFileName,
  selectChangePreview,
  shouldAutoExpandTurnChanges,
  summarizeChangeScopes,
} from "./turnChangePresentation";

const file = (path: string): TurnChangeFile => ({
  path,
  added: 1,
  removed: 0,
  kind: "edited",
  counts_unknown: false,
});

describe("path helpers", () => {
  it("splits a path into name and directory across separator styles", () => {
    expect(changedFileName("src/features/chat/a.ts")).toBe("a.ts");
    expect(changedFileDirectory("src/features/chat/a.ts")).toBe("src/features/chat");
    expect(changedFileName("src\\features\\a.ts")).toBe("a.ts");
    expect(changedFileDirectory("README.md")).toBe("");
  });

  it("treats a root-level file as its own scope", () => {
    expect(changeScope("src/a.ts")).toBe("src");
    expect(changeScope("README.md")).toBe("root");
  });
});

describe("selectChangePreview", () => {
  it("shows one file per scope so the preview conveys reach", () => {
    const preview = selectChangePreview([
      file("src/a.ts"),
      file("src/b.ts"),
      file("src/c.ts"),
      file("docs/d.md"),
      file("tests/e.test.ts"),
    ]);

    expect(preview.map((entry) => entry.path)).toEqual(["src/a.ts", "docs/d.md", "tests/e.test.ts"]);
  });

  it("tops up in order when a turn worked inside a single directory", () => {
    const preview = selectChangePreview([file("src/a.ts"), file("src/b.ts"), file("src/c.ts")]);
    expect(preview.map((entry) => entry.path)).toEqual(["src/a.ts", "src/b.ts", "src/c.ts"]);
  });
});

describe("summarizeChangeScopes", () => {
  it("ranks scopes by file count and keeps first-seen order on ties", () => {
    expect(
      summarizeChangeScopes([file("src/a.ts"), file("docs/b.md"), file("src/c.ts"), file("tests/d.ts")]),
    ).toEqual([
      { label: "src", file_count: 2 },
      { label: "docs", file_count: 1 },
      { label: "tests", file_count: 1 },
    ]);
  });
});

describe("shouldAutoExpandTurnChanges", () => {
  it("expands a small change set and collapses a large one", () => {
    const small = Array.from({ length: TURN_CHANGE_AUTO_EXPAND_FILE_LIMIT }, (_, index) => file(`src/${index}.ts`));
    const large = Array.from({ length: TURN_CHANGE_AUTO_EXPAND_FILE_LIMIT + 1 }, (_, index) => file(`src/${index}.ts`));

    expect(shouldAutoExpandTurnChanges(small)).toBe(true);
    expect(shouldAutoExpandTurnChanges(large)).toBe(false);
    expect(shouldAutoExpandTurnChanges([])).toBe(false);
  });
});
