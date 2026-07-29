import fs from "node:fs";
import path from "node:path";
import { expect, test, type Locator, type WebSocketRoute } from "@playwright/test";

function remoteActionBody(body: unknown): {
  action?: string;
  target?: string;
  prompt?: string;
  input_mode?: string;
} {
  return typeof body === "object" && body !== null ? body : {};
}

test("remote mobile shell renders team-ordered watchlist and opens agent detail", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });

  const screenshotDir = process.env.WARDIAN_MOBILE_PWA_PARITY_SCREENSHOT_DIR;
  if (screenshotDir) fs.mkdirSync(screenshotDir, { recursive: true });
  const captureFeatureScreenshot = async (name: string, locator: Locator) => {
    if (!screenshotDir) return;
    await locator.screenshot({ path: path.join(screenshotDir, name), animations: "disabled" });
  };

  const actionRequests: Array<{ headers: Record<string, string>; body: unknown }> = [];
  let statusStream: WebSocketRoute | null = null;
  let terminalStream: WebSocketRoute | null = null;
  const terminalInputs: string[] = [];
  const recoveryScrollback = Array.from(
    { length: 160 },
    (_, index) => `recovery scrollback line ${String(index + 1).padStart(3, "0")}`,
  );

  await page.route("**/remote/api/session", async (route) => {
    await route.fulfill({
      contentType: "application/json",
      body: JSON.stringify({
        csrf_nonce: "csrf-e2e",
        expires_at: "2099-05-21T08:05:00.000Z",
        absolute_expires_at: "2099-05-21T20:00:00.000Z",
      }),
    });
  });
  await page.route("**/remote/api/agents", async (route) => {
    await route.fulfill({
      contentType: "application/json",
      body: JSON.stringify({
        agents: [
          {
            session_id: "agent-2",
            session_name: "Remote Reviewer",
            agent_class: "Reviewer",
            provider: "claude",
            workspace: "<absolute-workspace-path>",
            status: "Processing",
            latest_text: null,
          },
          {
            session_id: "agent-1",
            session_name: "Remote Coder",
            agent_class: "Coder",
            provider: "opencode",
            workspace: "<absolute-workspace-path>",
            status: "Processing",
            latest_text: "Working",
          },
        ],
      }),
    });
  });
  await page.route("**/remote/api/watchlists", async (route) => {
    await route.fulfill({
      contentType: "application/json",
      body: JSON.stringify({
        watchlists: [{ id: "main", name: "Main", entries: [{ type: "team", teamId: "team-1" }] }],
        teams: [{ id: "team-1", name: "Remote Team", agentIds: ["agent-2", "agent-1"] }],
        prefs: { columns: [], sort: null, preserve_team_grouping_when_sorted: false, collapsed_team_ids: [] },
      }),
    });
  });
  await page.route("**/remote/api/workflows", async (route) => {
    await route.fulfill({ contentType: "application/json", body: JSON.stringify({ workflows: [] }) });
  });
  await page.route("**/remote/api/agents/agent-1/chat", async (route) => {
    await route.fulfill({
      contentType: "application/json",
      body: JSON.stringify({
        events: [
          {
            id: "remote-user-message",
            session_id: "agent-1",
            provider: "opencode",
            kind: "message",
            role: "user",
            text: "Summarize the current implementation status.",
            title: null,
            status: null,
            turn_id: "turn-1",
            source: "provider_log",
            command: null,
            exit_code: null,
            path: null,
            language: null,
            created_at: "2099-05-21T07:59:00.000Z",
            sequence: 1,
            metadata: {},
          },
          {
            id: "remote-agent-message",
            session_id: "agent-1",
            provider: "opencode",
            kind: "message",
            role: "assistant",
            text: "The navigation workbench is implemented and the focused verification is passing.",
            title: null,
            status: null,
            turn_id: "turn-1",
            source: "provider_log",
            command: null,
            exit_code: null,
            path: null,
            language: null,
            created_at: "2099-05-21T08:00:00.000Z",
            sequence: 2,
            metadata: {},
          },
        ],
      }),
    });
  });
  await page.route("**/remote/api/ws-ticket", async (route) => {
    await route.fulfill({
      contentType: "application/json",
      body: JSON.stringify({
        ticket: "ws-ticket-e2e",
        expires_at: "2099-05-21T08:05:00.000Z",
      }),
    });
  });
  await page.routeWebSocket("**/remote/api/status-stream", async (ws) => {
    statusStream = ws;
    ws.onMessage(() => {});
  });
  await page.routeWebSocket("**/remote/api/agents/agent-1/terminal-stream", async (ws) => {
    terminalStream = ws;
    let seeded = false;
    let recoverySnapshotSent = false;
    let liveEventSent = false;
    ws.onMessage((message) => {
      const payload = JSON.parse(String(message));
      if (payload.type === "input") terminalInputs.push(String(payload.data ?? ""));
      if (!seeded) {
        expect(payload).toMatchObject({
          protocol_version: 2,
          ticket: "ws-ticket-e2e",
          cols: expect.any(Number),
          rows: expect.any(Number),
        });
        seeded = true;
        ws.send(
          JSON.stringify({
            type: "registered",
            protocol_version: 2,
            presentation: {
              presentation_id: "remote-e2e",
              client_kind: "remote",
              desired_geometry: { cols: 80, rows: 24 },
              visibility: "visible",
              render_state: "mounted",
              interaction_capability: "interactive",
              interaction_sequence: 1,
              requires_resync: false,
            },
            broker_state: {
              session_id: "agent-1",
              runtime_generation: 1,
              lease_epoch: 1,
              stream_sequence: 1,
              interaction_sequence: 1,
              geometry: { cols: 80, rows: 24 },
              owner_presentation_id: "desktop-e2e",
              pending_activation: null,
              runtime_state: "live",
            },
            initial_snapshot: {
              snapshot_id: "snapshot-e2e",
              session_id: "agent-1",
              runtime_generation: 1,
              sequence_barrier: 0,
              geometry: { cols: 80, rows: 24 },
              terminal_state_base64: Buffer.from("terminal ready from e2e", "utf8").toString("base64"),
              visible_grid: "terminal ready from e2e",
              scrollback: [],
            },
          }),
        );
        return;
      }
      if (payload.type === "request_events" && !recoverySnapshotSent) {
        recoverySnapshotSent = true;
        ws.send(JSON.stringify({
          type: "events",
          batch: {
            status: "gap",
            runtime_generation: 1,
            events: [],
            next_sequence: 10,
            available_from_sequence: 10,
            latest_sequence: 10,
            recovery_snapshot: {
              snapshot_id: "recovery-snapshot-e2e",
              session_id: "agent-1",
              runtime_generation: 1,
              sequence_barrier: 10,
              geometry: { cols: 80, rows: 24 },
              terminal_state_base64: Buffer.from("recovered current viewport", "utf8").toString("base64"),
              visible_grid: "recovered current viewport",
              scrollback: recoveryScrollback,
            },
          },
        }));
      } else if (payload.type === "request_events" && !liveEventSent) {
        liveEventSent = true;
        ws.send(JSON.stringify({
          type: "events",
          batch: {
            status: "events",
            runtime_generation: 1,
            events: [{
              type: "output",
              sequence: 11,
              runtime_generation: 1,
              bytes_base64: Buffer.from("Finished remote e2e update.", "utf8").toString("base64"),
            }],
            next_sequence: 11,
            available_from_sequence: 11,
            latest_sequence: 11,
            recovery_snapshot: null,
          },
        }));
      } else if (payload.type === "begin_activation") {
        ws.send(JSON.stringify({
          type: "activation_begin",
          result: {
            decision: {
              status: "accepted",
              reason: null,
              runtime_generation: 1,
              lease_epoch: 2,
              owner_presentation_id: "desktop-e2e",
            },
            activation_id: "activation-e2e",
            snapshot: {
              snapshot_id: "activation-snapshot-e2e",
              session_id: "agent-1",
              runtime_generation: 1,
              sequence_barrier: 1,
              geometry: { cols: 80, rows: 24 },
              terminal_state_base64: Buffer.from("terminal ready from e2e", "utf8").toString("base64"),
              visible_grid: "terminal ready from e2e",
              scrollback: [],
            },
            sequence_barrier: 1,
          },
        }));
      } else if (payload.type === "ack_activation") {
        ws.send(JSON.stringify({
          type: "activation_ack",
          result: {
            decision: {
              status: "accepted",
              reason: null,
              runtime_generation: 1,
              lease_epoch: 2,
              owner_presentation_id: "remote-e2e",
            },
            broker_state: {
              session_id: "agent-1",
              runtime_generation: 1,
              lease_epoch: 2,
              stream_sequence: 1,
              interaction_sequence: 2,
              geometry: { cols: 80, rows: 24 },
              owner_presentation_id: "remote-e2e",
              pending_activation: null,
              runtime_state: "live",
            },
            snapshot: null,
          },
        }));
      }
    });
  });
  await page.route("**/remote/api/agents/action", async (route) => {
    actionRequests.push({
      headers: route.request().headers(),
      body: JSON.parse(route.request().postData() ?? "{}"),
    });
    await route.fulfill({ contentType: "application/json", body: JSON.stringify({ ok: true }) });
  });

  await page.goto("/remote", { waitUntil: "domcontentloaded" });
  await expect(page.locator('[data-testid="remote-mobile-app"]')).toBeVisible();
  await expect(page.getByText("Remote Coder")).toBeVisible();
  await expect(page.locator('[data-testid="remote-watchlist-view"]')).toBeVisible();
  await expect(page.getByText("Remote Team")).toBeVisible();
  const rowNames = await page.locator('[data-testid="remote-watchlist-agent-row"]').allTextContents();
  expect(rowNames).toEqual([
    expect.stringContaining("Remote Reviewer"),
    expect.stringContaining("Remote Coder"),
  ]);
  await expect(page.getByRole("navigation", { name: "Remote sections" })).toBeVisible();

  await page.getByRole("button", { name: "Open remote settings" }).click();
  await expect(page.getByRole("heading", { name: "Settings" })).toBeVisible();
  await page.getByLabel("Theme").selectOption("dark");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await captureFeatureScreenshot("settings-view.png", page.locator("main"));
  await page.getByRole("button", { name: "Back to remote watchlist" }).click();
  await expect(page.locator('[data-testid="remote-watchlist-view"]')).toBeVisible();
  await expect(page.getByRole("button", { name: "Open broadcast prompt" })).toHaveCount(0);
  await expect(page.getByRole("textbox", { name: "Broadcast prompt" })).toHaveCount(0);
  await captureFeatureScreenshot("watchlist-no-broadcast.png", page.locator('[data-testid="remote-watchlist-view"]'));

  await page.getByRole("button", { name: "Open Remote Coder details" }).click();
  await expect(page.locator('[data-testid="remote-agent-detail"]')).toBeVisible();
  await expect(page.getByRole("button", { name: "Terminal", exact: true })).toHaveAttribute("aria-pressed", "true");
  await expect(page.getByText("recovered current viewport")).toBeVisible();
  await expect(page.getByRole("button", { name: "Take terminal control" })).toHaveCount(0);
  await captureFeatureScreenshot("lifecycle-actions.png", page.locator('[data-testid="remote-agent-detail"]'));
  await captureFeatureScreenshot("terminal-detail.png", page.locator('[data-testid="remote-agent-detail"]'));
  const terminalScreen = page.locator('[data-testid="remote-terminal-scroll-surface"] .xterm-screen');
  const terminalScrollThumb = page.locator(
    '[data-testid="remote-terminal-scroll-surface"] .scrollbar.vertical .slider',
  );
  const scrollTopBefore = await terminalScrollThumb.evaluate(
    (element) => Number.parseFloat((element as HTMLElement).style.top),
  );
  expect(scrollTopBefore).toBeGreaterThan(0);
  await terminalScreen.hover();
  await page.mouse.wheel(0, -2_000);
  await expect.poll(async () => terminalScrollThumb.evaluate(
    (element) => Number.parseFloat((element as HTMLElement).style.top),
  )).toBeLessThan(scrollTopBefore);
  await captureFeatureScreenshot("terminal-recovery-scrollback.png", page.locator('[data-testid="remote-agent-detail"]'));

  await expect.poll(() => terminalStream !== null).toBe(true);
  terminalStream?.send(JSON.stringify({
    type: "events",
    batch: {
      status: "events",
      runtime_generation: 1,
      events: [{
        type: "output",
        sequence: 12,
        runtime_generation: 1,
        bytes_base64: Buffer.from(
          "\u001b[?1049h\u001b[?1000h\u001b[?1006hAlternate terminal scroll target",
          "utf8",
        ).toString("base64"),
      }],
      next_sequence: 12,
      available_from_sequence: 12,
      latest_sequence: 12,
      recovery_snapshot: null,
    },
  }));
  await expect(page.getByText("Alternate terminal scroll target")).toBeVisible();
  const terminalInputCountBeforeTouch = terminalInputs.length;
  await page.locator('[data-testid="remote-terminal-scroll-surface"]').dispatchEvent("touchstart", {
    touches: [{ identifier: 0, clientX: 180, clientY: 420 }],
  });
  await page.locator('[data-testid="remote-terminal-scroll-surface"]').dispatchEvent("touchmove", {
    touches: [{ identifier: 0, clientX: 180, clientY: 360 }],
  });
  await expect.poll(() => terminalInputs.length).toBeGreaterThan(terminalInputCountBeforeTouch);
  expect(terminalInputs.at(-1)).toMatch(/^\u001b\[<6[45];/);

  await expect.poll(() => statusStream !== null).toBe(true);
  statusStream?.send(
    JSON.stringify({
      type: "agent_status",
      agents: [
        {
          session_id: "agent-2",
          session_name: "Remote Reviewer",
          agent_class: "Reviewer",
          provider: "claude",
          workspace: "<absolute-workspace-path>",
          status: "Processing",
          latest_text: null,
        },
        {
          session_id: "agent-1",
          session_name: "Remote Coder",
          agent_class: "Coder",
          provider: "opencode",
          workspace: "<absolute-workspace-path>",
          status: "Idle",
          latest_text: "Ready",
        },
      ],
    }),
  );
  await page.getByRole("button", { name: "Chat", exact: true }).click();
  await expect(page.getByLabel("user message")).toHaveClass(/\bw-full\b/);
  await expect(page.getByLabel("assistant message")).toHaveClass(/\bw-full\b/);
  await captureFeatureScreenshot("chat-full-width.png", page.locator('[data-testid="remote-agent-detail"]'));
  await page.getByLabel("Prompt Remote Coder").fill("status please");
  await page.getByRole("button", { name: "Send prompt" }).click();

  await expect
    .poll(() => actionRequests.filter(({ body }) => remoteActionBody(body).prompt === "status please").length)
    .toBe(1);
  const chatPromptRequest = actionRequests.find(({ body }) => remoteActionBody(body).prompt === "status please");
  expect(chatPromptRequest).toMatchObject({
    headers: {
      "x-wardian-csrf": "csrf-e2e",
    },
    body: {
      action: "send_prompt",
      target: "agent-1",
      prompt: "status please",
    },
  });

  await page.getByRole("button", { name: "Back to remote agents" }).click();
  await page.getByRole("button", { name: "Inbox" }).click();
  await expect(page.getByText("Agent task completed")).toBeVisible();
  await expect(page.getByText("Finished remote e2e update.")).toBeVisible();
  await captureFeatureScreenshot("inbox-summary.png", page.locator("main"));
});
