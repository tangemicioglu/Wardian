import assert from "node:assert/strict";
import test from "node:test";

import {
  resolveAgentSessionTerminalPresentationId,
  resolveAgentTerminalPresentationId,
} from "../lib/terminal-debug.mjs";

class FakeElement {
  constructor(attributes = {}, preview = null) {
    this.attributes = attributes;
    this.preview = preview;
  }

  getAttribute(name) {
    return this.attributes[name] ?? null;
  }

  querySelector(selector) {
    if (selector.includes('data-testid="agent-terminal-host"')) return null;
    if (selector.includes('data-zellij-presentation="preview"')) return this.preview;
    return null;
  }

  dispatchEvent() {
    return true;
  }
}

function terminalHost(sessionId, presentationId) {
  return new FakeElement({
    "data-terminal-session-id": sessionId,
    "data-terminal-presentation-id": presentationId,
  });
}

function passiveRoot(attributes = {}) {
  return new FakeElement(attributes, new FakeElement({ "aria-disabled": "false" }));
}

function fakeDriver({ cards = new Map(), panels = [], singletonHosts = [], presentationIds = [] }) {
  return {
    async executeScript(callback, ...args) {
      const previous = {
        CSS: globalThis.CSS,
        document: globalThis.document,
        HTMLElement: globalThis.HTMLElement,
        PointerEvent: globalThis.PointerEvent,
        window: globalThis.window,
      };
      globalThis.CSS = { escape: (value) => value };
      globalThis.HTMLElement = FakeElement;
      globalThis.PointerEvent = class {};
      globalThis.document = {
        getElementById: (id) => cards.get(id) ?? null,
        querySelectorAll: (selector) => (
          selector.includes('data-surface-type="agent-session"') ? panels : singletonHosts
        ),
      };
      globalThis.window = {
        __wardianTerminalDebug: { presentationIds: () => presentationIds },
      };
      try {
        return callback(...args);
      } finally {
        Object.assign(globalThis, previous);
      }
    },
    async wait(probe, _timeoutMs, message) {
      const result = await probe();
      if (!result) throw new Error(message);
      return result;
    },
  };
}

test("resolves the singleton renderer for one exact passive Agents card", async () => {
  const sessionId = "session-a";
  const presentationId = "desktop:zellij-habitat-terminal";
  const driver = fakeDriver({
    cards: new Map([[`agent-card-${sessionId}`, passiveRoot()]]),
    singletonHosts: [terminalHost(sessionId, presentationId)],
    presentationIds: [presentationId],
  });

  assert.equal(
    await resolveAgentTerminalPresentationId(driver, sessionId),
    presentationId,
  );
});

test("fails closed when the singleton host is ambiguous", async () => {
  const sessionId = "session-a";
  const driver = fakeDriver({
    cards: new Map([[`agent-card-${sessionId}`, passiveRoot()]]),
    singletonHosts: [
      terminalHost(sessionId, "presentation-a"),
      terminalHost(sessionId, "presentation-b"),
    ],
    presentationIds: ["presentation-a", "presentation-b"],
  });

  await assert.rejects(
    resolveAgentTerminalPresentationId(driver, sessionId, 1),
    /Timed out resolving the terminal presentation for agent session-a/,
  );
});

test("agent-session resolution requires an exact surface before using the singleton", async () => {
  const sessionId = "session-a";
  const presentationId = "desktop:zellij-habitat-terminal";
  const panels = [
    passiveRoot({ "data-resource-key": sessionId, "data-surface-id": "surface-a" }),
    passiveRoot({ "data-resource-key": sessionId, "data-surface-id": "surface-b" }),
  ];
  const driver = fakeDriver({
    panels,
    singletonHosts: [terminalHost(sessionId, presentationId)],
    presentationIds: [presentationId],
  });

  await assert.rejects(
    resolveAgentSessionTerminalPresentationId(driver, sessionId, { timeoutMs: 1 }),
    /Timed out resolving the terminal presentation for agent-session session-a/,
  );
  assert.equal(
    await resolveAgentSessionTerminalPresentationId(driver, sessionId, {
      surfaceId: "surface-b",
      timeoutMs: 1,
    }),
    presentationId,
  );
});
