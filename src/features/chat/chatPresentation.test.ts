import { describe, expect, it } from "vitest";

import type { AgentChatEvent } from "../../types";
import { toActivityBlock } from "../grid/activityBlocks";
import {
  isProcessingAgentStatus,
  resolvedActivityTone,
  shouldShowChatEvent,
  sortTranscriptEvents,
} from "./chatPresentation";

const event = (overrides: Partial<AgentChatEvent>): AgentChatEvent => ({
  id: "event-1",
  session_id: "agent-1",
  provider: "codex",
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

describe("resolvedActivityTone", () => {
  it("keeps the in-flight tone while the agent is working", () => {
    expect(resolvedActivityTone("processing", true)).toBe("processing");
  });

  it("demotes a stranded in-flight tone to neutral once the agent stops", () => {
    // A tool call the provider never resolved keeps status "running" forever.
    // Painting it as in-flight after the agent went idle asserts work that is
    // demonstrably not happening.
    expect(resolvedActivityTone("processing", false)).toBe("neutral");
  });

  it("leaves settled tones untouched regardless of agent state", () => {
    (["success", "warning", "error", "neutral"] as const).forEach((tone) => {
      expect(resolvedActivityTone(tone, false)).toBe(tone);
      expect(resolvedActivityTone(tone, true)).toBe(tone);
    });
  });

  it("demotes an unresolved running tool call derived from a real event", () => {
    const block = toActivityBlock(event({ command: "cargo test", status: "running" }));
    expect(block.tone).toBe("processing");
    expect(resolvedActivityTone(block.tone, false)).toBe("neutral");
  });
});

describe("isProcessingAgentStatus", () => {
  it("recognizes the processing and running status labels", () => {
    expect(isProcessingAgentStatus("Processing")).toBe(true);
    expect(isProcessingAgentStatus("running")).toBe(true);
  });

  it("treats idle, absent, and action-required statuses as not working", () => {
    expect(isProcessingAgentStatus("Idle")).toBe(false);
    expect(isProcessingAgentStatus(null)).toBe(false);
    expect(isProcessingAgentStatus(undefined)).toBe(false);
    expect(isProcessingAgentStatus("Action Required")).toBe(false);
  });
});

describe("shouldShowChatEvent", () => {
  it("hides an anonymous in-flight tool call that carries no work", () => {
    expect(shouldShowChatEvent(event({ status: "running" }))).toBe(false);
  });

  it("keeps an in-flight tool call once it has a recognizable identity", () => {
    expect(shouldShowChatEvent(event({ status: "running", metadata: { tool_name: "Edit" } }))).toBe(true);
  });

  it("keeps the synthetic thinking indicator but drops ordinary status events", () => {
    expect(shouldShowChatEvent(event({ kind: "status", metadata: { chat_thinking_indicator: true } }))).toBe(true);
    expect(shouldShowChatEvent(event({ kind: "status", status: "succeeded" }))).toBe(false);
    expect(shouldShowChatEvent(event({ kind: "status", status: "failed" }))).toBe(true);
  });
});

describe("sortTranscriptEvents", () => {
  it("orders by sequence and falls back to timestamps", () => {
    const sorted = sortTranscriptEvents([
      event({ id: "b", sequence: 2 }),
      event({ id: "a", sequence: 1 }),
      event({ id: "c", sequence: 3 }),
    ]);
    expect(sorted.map((item) => item.id)).toEqual(["a", "b", "c"]);

    const byTime = sortTranscriptEvents([
      event({ id: "late", created_at: "2026-08-08T10:00:00.000Z" }),
      event({ id: "early", created_at: "2026-08-08T09:00:00.000Z" }),
    ]);
    expect(byTime.map((item) => item.id)).toEqual(["early", "late"]);
  });
});
