import { describe, expect, it } from "vitest";

import type { AgentChatEvent } from "../../types";
import { derivePresentedChatRows } from "../grid/workLogPresentation";
import { withTurnChangeSummaries, type TurnChangeSummaryRow } from "./chatTurns";

const event = (overrides: Partial<AgentChatEvent>): AgentChatEvent => ({
  id: "event-1",
  session_id: "agent-1",
  provider: "claude",
  kind: "tool_call",
  role: null,
  text: null,
  title: null,
  status: null,
  turn_id: null,
  source: null,
  command: null,
  exit_code: null,
  path: null,
  language: null,
  created_at: null,
  sequence: null,
  metadata: {},
  ...overrides,
});

const userMessage = (id: string, sequence: number) =>
  event({ id, kind: "message", role: "user", text: "do the thing", sequence });

const editCall = (id: string, sequence: number, path: string, oldText: string, newText: string) =>
  event({
    id,
    kind: "tool_call",
    title: "Edit",
    status: "running",
    sequence,
    metadata: { tool_name: "Edit", tool_input: { file_path: path, old_string: oldText, new_string: newText } },
  });

function summaries(events: AgentChatEvent[]): TurnChangeSummaryRow[] {
  return withTurnChangeSummaries(derivePresentedChatRows(events)).filter(
    (row): row is TurnChangeSummaryRow => row.kind === "turn_change_summary",
  );
}

describe("withTurnChangeSummaries", () => {
  it("summarizes the files a turn changed with exact line counts", () => {
    const rows = summaries([
      userMessage("user-1", 1),
      editCall("edit-1", 2, "src/a.ts", "one\ntwo", "ONE\nTWO"),
      event({ id: "assistant-1", kind: "message", role: "assistant", text: "Done.", sequence: 3 }),
    ]);

    expect(rows).toHaveLength(1);
    expect(rows[0].files).toEqual([
      { path: "src/a.ts", added: 2, removed: 2, kind: "edited", counts_unknown: false },
    ]);
    expect(rows[0].added).toBe(2);
    expect(rows[0].removed).toBe(2);
  });

  it("emits nothing for a turn that only talked", () => {
    expect(
      summaries([
        userMessage("user-1", 1),
        event({ id: "assistant-1", kind: "message", role: "assistant", text: "No changes needed.", sequence: 2 }),
      ]),
    ).toEqual([]);
  });

  it("splits on user messages so each turn reports only its own work", () => {
    const rows = summaries([
      userMessage("user-1", 1),
      editCall("edit-1", 2, "src/a.ts", "one", "ONE"),
      userMessage("user-2", 3),
      editCall("edit-2", 4, "src/b.ts", "two", "TWO"),
    ]);

    expect(rows).toHaveLength(2);
    expect(rows[0].files.map((file) => file.path)).toEqual(["src/a.ts"]);
    expect(rows[1].files.map((file) => file.path)).toEqual(["src/b.ts"]);
  });

  it("accumulates repeated edits to one file within a turn", () => {
    const rows = summaries([
      userMessage("user-1", 1),
      editCall("edit-1", 2, "src/a.ts", "one", "ONE"),
      editCall("edit-2", 3, "src/a.ts", "two", "TWO"),
    ]);

    expect(rows[0].files).toHaveLength(1);
    expect(rows[0].files[0]).toMatchObject({ path: "src/a.ts", added: 2, removed: 2 });
  });

  it("marks a file the turn created", () => {
    const rows = summaries([
      userMessage("user-1", 1),
      event({
        id: "write-1",
        kind: "tool_call",
        title: "Write",
        status: "running",
        sequence: 2,
        metadata: { tool_name: "Write", tool_input: { file_path: "docs/new.md", content: "a\nb" } },
      }),
    ]);

    expect(rows[0].files[0]).toMatchObject({ path: "docs/new.md", kind: "created", added: 2, removed: 0 });
  });

  it("reports a path-only write without inventing line counts", () => {
    const rows = summaries([
      userMessage("user-1", 1),
      event({
        id: "tool-1",
        kind: "tool_result",
        title: "apply_patch",
        text: "patch applied",
        sequence: 2,
        metadata: { files_written: ["src/legacy.ts"] },
      }),
    ]);

    expect(rows[0].files[0]).toMatchObject({ path: "src/legacy.ts", counts_unknown: true, added: 0, removed: 0 });
  });

  it("does not count files a tool merely read as changes", () => {
    expect(
      summaries([
        userMessage("user-1", 1),
        event({
          id: "read-1",
          kind: "tool_call",
          title: "Read",
          sequence: 2,
          metadata: { tool_name: "Read", file_path: "src/a.ts", files_read: ["src/a.ts"] },
        }),
      ]),
    ).toEqual([]);
  });

  it("summarizes work that precedes the first user message", () => {
    const rows = summaries([editCall("edit-1", 1, "src/a.ts", "one", "ONE")]);
    expect(rows).toHaveLength(1);
    expect(rows[0].files[0].path).toBe("src/a.ts");
  });

  it("places the summary after the turn it describes", () => {
    const rows = withTurnChangeSummaries(
      derivePresentedChatRows([
        userMessage("user-1", 1),
        editCall("edit-1", 2, "src/a.ts", "one", "ONE"),
        userMessage("user-2", 3),
      ]),
    );

    const kinds = rows.map((row) => (row.kind === "event" ? `event:${row.event.id}` : row.kind));
    expect(kinds).toEqual(["event:user-1", "event:edit-1", "turn_change_summary", "event:user-2"]);
  });
});
