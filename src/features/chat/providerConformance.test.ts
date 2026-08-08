import { describe, expect, it } from "vitest";

import type { AgentChatEvent } from "../../types";
import { derivePresentedChatRows } from "../grid/workLogPresentation";
import { liveApprovalEventId, shouldShowChatEvent, sortTranscriptEvents, toolPatchText } from "./chatPresentation";
import { withTurnChangeSummaries, type TurnChangeSummaryRow } from "./chatTurns";
import { structuredEditFromEvent } from "./structuredEdit";

/**
 * Cross-provider conformance for the turn change surface.
 *
 * Every event below mirrors what `providers/chat_transcript.rs` actually emits
 * for that provider — the metadata keys, the status values, and the places a
 * file path can hide all differ per provider, and the Rust tests in that module
 * pin the same shapes. The point of this suite is that a feature which works on
 * Claude is not assumed to work on Codex: each provider gets the same
 * assertions against its own event shape.
 *
 * Where a provider genuinely cannot support a feature, the gap is asserted
 * rather than skipped, so a later normalizer change that closes it fails here
 * and gets noticed.
 */

let sequence = 0;

function event(overrides: Partial<AgentChatEvent>): AgentChatEvent {
  sequence += 1;
  return {
    id: `event-${sequence}`,
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
    sequence,
    metadata: {},
    ...overrides,
  };
}

function userMessage(provider: string, text: string): AgentChatEvent {
  return event({ provider, kind: "message", role: "user", text, metadata: { raw_type: "user" } });
}

/** Runs a transcript through the full view pipeline and returns the change rows. */
function turnChanges(events: AgentChatEvent[]): TurnChangeSummaryRow[] {
  const rows = withTurnChangeSummaries(
    derivePresentedChatRows(sortTranscriptEvents(events).filter(shouldShowChatEvent)),
  );
  return rows.filter((row): row is TurnChangeSummaryRow => row.kind === "turn_change_summary");
}

/**
 * One edit event per provider, in that provider's own normalized shape.
 *
 * Claude and OpenCode describe the edit in structured input; Codex emits a
 * patch string; Antigravity reports only the path it wrote. Gemini reports
 * neither, which is why it has no entry here.
 */
const editEvents = {
  claude: () =>
    event({
      provider: "claude",
      title: "Edit",
      status: "running",
      metadata: {
        raw_type: "Edit",
        tool_name: "Edit",
        file_path: "src/app.ts",
        files_written: ["src/app.ts"],
        tool_input: { file_path: "src/app.ts", old_string: "const a = 1;", new_string: "const a = 2;" },
      },
    }),
  codex: () =>
    event({
      provider: "codex",
      title: "apply_patch",
      status: "running",
      metadata: {
        raw_type: "custom_tool_call",
        tool_name: "apply_patch",
        tool_input_text:
          "*** Begin Patch\n*** Update File: src/app.ts\n@@\n-const a = 1;\n+const a = 2;\n*** End Patch",
      },
    }),
  opencode: () =>
    event({
      provider: "opencode",
      title: "edit",
      status: "running",
      metadata: {
        raw_type: "edit",
        tool_name: "edit",
        file_path: "src/app.ts",
        files_written: ["src/app.ts"],
        tool_input: { filePath: "src/app.ts", oldString: "const a = 1;", newString: "const a = 2;" },
      },
    }),
  antigravity: () =>
    event({
      provider: "antigravity",
      title: "Edit file",
      status: "running",
      metadata: {
        raw_type: "replace_file_content",
        tool_name: "replace_file_content",
        file_path: "src/app.ts",
        files_written: ["src/app.ts"],
        tool_input: { TargetFile: "src/app.ts" },
      },
    }),
};

describe("turn segmentation across providers", () => {
  // Turns split on user messages. Codex, Claude, Gemini, and Antigravity emit
  // them from their own logs; OpenCode's come from its SQLite database rather
  // than the stream, but arrive as the same normalized shape.
  it.each(["claude", "codex", "gemini", "antigravity", "opencode"])("splits a %s transcript on user messages", (provider) => {
    const changes = turnChanges([
      userMessage(provider, "first ask"),
      event({
        provider,
        title: "Write",
        metadata: { tool_name: "Write", files_written: ["a.ts"] },
      }),
      userMessage(provider, "second ask"),
      event({
        provider,
        title: "Write",
        metadata: { tool_name: "Write", files_written: ["b.ts"] },
      }),
    ]);

    expect(changes).toHaveLength(2);
    expect(changes[0].files.map((file) => file.path)).toEqual(["a.ts"]);
    expect(changes[1].files.map((file) => file.path)).toEqual(["b.ts"]);
  });
});

describe("file changes are recovered from every provider that reports them", () => {
  it("reads Claude structured input with exact counts", () => {
    const [summary] = turnChanges([userMessage("claude", "go"), editEvents.claude()]);
    expect(summary.files).toEqual([
      { path: "src/app.ts", added: 1, removed: 1, kind: "edited", counts_unknown: false },
    ]);
  });

  it("reads OpenCode structured input despite its camelCase keys", () => {
    // OpenCode names the same fields filePath/oldString/newString. Before the
    // normalizer preserved `part.input` this produced nothing at all.
    const [summary] = turnChanges([userMessage("opencode", "go"), editEvents.opencode()]);
    expect(summary.files).toEqual([
      { path: "src/app.ts", added: 1, removed: 1, kind: "edited", counts_unknown: false },
    ]);
  });

  it("reads Codex patch text held in tool input", () => {
    // apply_patch is Codex's only edit path and it carries no event text, so
    // the patch has to come out of metadata or the turn shows nothing.
    const [summary] = turnChanges([userMessage("codex", "go"), editEvents.codex()]);
    expect(summary.files).toEqual([
      { path: "src/app.ts", added: 1, removed: 1, kind: "edited", counts_unknown: false },
    ]);
  });

  it("reports an Antigravity write as touched without inventing counts", () => {
    // Antigravity names the file it wrote but never the content, so the card
    // shows the path with unknown counts rather than a fabricated zero.
    const [summary] = turnChanges([userMessage("antigravity", "go"), editEvents.antigravity()]);
    expect(summary.files).toEqual([
      { path: "src/app.ts", added: 0, removed: 0, kind: "edited", counts_unknown: true },
    ]);
    expect(summary.added).toBe(0);
  });

  it("produces no change row for Gemini, which reports no file writes", () => {
    // Gemini's tool_use records carry a tool name and nothing else. Asserting
    // the gap keeps it visible: if the normalizer starts supplying paths, this
    // test fails and the card can be trusted for Gemini too.
    const geminiTool = event({
      provider: "gemini",
      title: "replace",
      status: "action_required",
      metadata: { raw_type: "replace", tool_name: "replace" },
    });
    expect(turnChanges([userMessage("gemini", "go"), geminiTool])).toHaveLength(0);
  });
});

describe("whole-file creates across providers", () => {
  it.each([
    ["claude", "Write", { file_path: "new.ts", content: "line one\nline two" }],
    ["opencode", "write", { filePath: "new.ts", content: "line one\nline two" }],
  ])("marks a %s %s as a created file", (provider, toolName, input) => {
    // The create is recognized by tool name, so a provider that omits
    // metadata.tool_name must still resolve it from the title.
    const created = event({
      provider,
      title: toolName,
      metadata: { raw_type: toolName, tool_input: input },
    });

    const edit = structuredEditFromEvent(created);
    expect(edit?.kind).toBe("create");
    expect(edit?.added).toBe(2);
    expect(edit?.file_path).toBe("new.ts");
  });

  it("resolves a create when the provider omits metadata.tool_name", () => {
    // Claude's normalizer historically labelled the tool only in the event
    // title, which silently disabled create detection.
    const titleOnly = event({
      provider: "claude",
      title: "Write",
      metadata: { raw_type: "Write", tool_input: { file_path: "new.ts", content: "only line" } },
    });
    expect(structuredEditFromEvent(titleOnly)?.kind).toBe("create");
  });
});

describe("approval liveness across providers", () => {
  // Providers disagree about which events count as approvals: Codex raises a
  // dedicated approval event, Claude a permission_request, and Gemini marks
  // every tool call action_required. In all cases exactly one row may be live.
  const approvals: Array<[string, AgentChatEvent[]]> = [
    [
      "codex",
      [
        event({ provider: "codex", kind: "approval", status: "succeeded", title: "Approval required" }),
        event({ provider: "codex", kind: "approval", status: "action_required", title: "Approval required" }),
      ],
    ],
    [
      "claude",
      [
        event({ provider: "claude", kind: "approval", status: "succeeded", title: "Bash" }),
        event({ provider: "claude", kind: "approval", status: "action_required", title: "Bash" }),
      ],
    ],
    [
      "gemini",
      [
        event({ provider: "gemini", kind: "tool_call", status: "action_required", title: "read_file" }),
        event({ provider: "gemini", kind: "tool_call", status: "action_required", title: "write_file" }),
      ],
    ],
    [
      "antigravity",
      [
        event({ provider: "antigravity", kind: "tool_call", status: "succeeded", title: "Run command" }),
        event({ provider: "antigravity", kind: "approval", status: "action_required", title: "Approval required" }),
      ],
    ],
  ];

  it.each(approvals)("makes only the newest %s approval actionable", (_provider, events) => {
    const sorted = sortTranscriptEvents(events);
    expect(liveApprovalEventId(sorted)).toBe(sorted[sorted.length - 1].id);
  });

  it.each(approvals)("makes no %s approval actionable once none is pending", (_provider, events) => {
    const settled = events.map((candidate) => ({ ...candidate, status: "succeeded" as const }));
    expect(liveApprovalEventId(sortTranscriptEvents(settled))).toBeNull();
  });
});

describe("patch text detection", () => {
  it("accepts both patch dialects and rejects ordinary tool arguments", () => {
    const codexPatch = event({ metadata: { tool_input_text: "*** Begin Patch\n*** Update File: a.ts\n*** End Patch" } });
    const gitPatch = event({ metadata: { tool_input_text: "diff --git a/a.ts b/a.ts\n@@\n-x\n+y" } });
    const arguments_ = event({ metadata: { tool_input_text: '{"query":"needle","path":"src"}' } });

    expect(toolPatchText(codexPatch)).toContain("*** Update File");
    expect(toolPatchText(gitPatch)).toContain("diff --git");
    expect(toolPatchText(arguments_)).toBeNull();
  });

  it("counts a multi-file Codex patch per file", () => {
    const multi = event({
      provider: "codex",
      title: "apply_patch",
      metadata: {
        tool_name: "apply_patch",
        tool_input_text: [
          "*** Begin Patch",
          "*** Update File: src/a.ts",
          "-old",
          "+new",
          "+extra",
          "*** Add File: src/b.ts",
          "+created",
          "*** End Patch",
        ].join("\n"),
      },
    });

    const [summary] = turnChanges([userMessage("codex", "go"), multi]);
    expect(summary.files).toEqual([
      { path: "src/a.ts", added: 2, removed: 1, kind: "edited", counts_unknown: false },
      { path: "src/b.ts", added: 1, removed: 0, kind: "created", counts_unknown: false },
    ]);
  });
});

describe("row visibility across providers", () => {
  it("keeps every provider's identifiable tool call", () => {
    const rows = [
      event({ provider: "claude", title: "Bash", status: "running", metadata: { tool_name: "Bash" } }),
      event({ provider: "codex", title: "shell_command", status: "running", metadata: { tool_name: "shell_command" } }),
      event({ provider: "gemini", title: "read_file", status: "action_required", metadata: { tool_name: "read_file" } }),
      event({ provider: "antigravity", title: "Run command", status: "running", metadata: { tool_name: "run_command" } }),
      event({ provider: "opencode", title: "bash", status: "running", metadata: { tool_name: "bash" } }),
    ];
    expect(rows.every(shouldShowChatEvent)).toBe(true);
  });

  it("still drops an anonymous in-flight keepalive", () => {
    // The identity fallback used for create detection must not leak into
    // visibility, or every provider keepalive earns a row.
    expect(shouldShowChatEvent(event({ status: "running", metadata: { raw_type: "function_call" } }))).toBe(false);
  });
});
