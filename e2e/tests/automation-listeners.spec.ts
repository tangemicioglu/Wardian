import { expect, test, type Page } from "@playwright/test";
import { openAutomationEditor } from "../fixtures/workbench";
import { mkdir } from "node:fs/promises";

const screenshotDirectory = "e2e/screenshots/automation-listeners/2026-09-05T03-40-00Z";
const fixedBrowserTime = "2026-09-05T16:00:00.000Z";

test.use({ locale: "en-US", timezoneId: "America/New_York" });

const emptyRuntime = {
  armed: true,
  arm_error: null,
  last_fire_epoch_ms: null,
  last_run_status: null,
  last_run_error: null,
  last_rejection: null,
  fire_count: 0,
  recent_fire_epoch_ms: [],
  disabled_reason: null,
  poll_fingerprint: null,
  next_poll_epoch_ms: null,
  consecutive_failures: 0,
};

const listenerFixtures = [
  {
    id: "listener-file",
    blueprint_id: "code-review",
    name: "Source audit",
    enabled: true,
    trigger: {
      type: "file_watch",
      path: "/workspace/repo",
      recursive: true,
      patterns: ["**/*.rs"],
      ignore: [],
      events: [],
      debounce_ms: 500,
    },
    provider: null,
    workspace: null,
    input: {},
    bindings: {},
    assignments: {},
    overlap: null,
    has_secret: false,
    webhook_url: null,
    runtime: {
      ...emptyRuntime,
      fire_count: 12,
      last_fire_epoch_ms: Date.parse("2026-09-05T15:42:00.000Z"),
      last_run_status: "completed",
    },
  },
  {
    id: "listener-hook",
    blueprint_id: "deploy-check",
    name: "CI results",
    enabled: true,
    trigger: {
      type: "webhook",
      path_segment: "ci",
      auth: "hmac_sha256",
      signature_header: null,
      max_body_bytes: 262144,
    },
    provider: null,
    workspace: null,
    input: {},
    bindings: {},
    assignments: {},
    overlap: null,
    has_secret: true,
    webhook_url: "http://127.0.0.1:8787/hooks/ci",
    runtime: {
      ...emptyRuntime,
      fire_count: 3,
      last_fire_epoch_ms: Date.parse("2026-09-05T14:10:00.000Z"),
      last_rejection: {
        reason: "delivery credential did not match the listener secret",
        at_epoch_ms: Date.parse("2026-09-05T15:00:00.000Z"),
      },
    },
  },
  {
    id: "listener-poll",
    blueprint_id: "release-notes",
    name: "Upstream releases",
    enabled: true,
    trigger: {
      type: "web_poll",
      url: "https://api.example.invalid/repos/acme/tool/releases",
      interval_seconds: 900,
      method: "get",
      headers: {},
      fingerprint: "json_pointer",
      json_pointer: "/0/tag_name",
      regex: null,
      max_body_bytes: 1048576,
    },
    provider: null,
    workspace: null,
    input: {},
    bindings: {},
    assignments: {},
    overlap: null,
    has_secret: false,
    webhook_url: null,
    runtime: {
      ...emptyRuntime,
      fire_count: 2,
      last_fire_epoch_ms: Date.parse("2026-09-04T09:00:00.000Z"),
      poll_fingerprint: 'json:"v4.11.2"',
    },
  },
  {
    id: "listener-runaway",
    blueprint_id: "code-review",
    name: "Build output watch",
    enabled: true,
    trigger: {
      type: "file_watch",
      path: "/workspace/repo/out",
      recursive: true,
      patterns: [],
      ignore: [],
      events: [],
      debounce_ms: 500,
    },
    provider: null,
    workspace: null,
    input: {},
    bindings: {},
    assignments: {},
    overlap: null,
    has_secret: false,
    webhook_url: null,
    runtime: {
      ...emptyRuntime,
      armed: false,
      fire_count: 21,
      last_fire_epoch_ms: Date.parse("2026-09-05T15:59:00.000Z"),
      disabled_reason:
        "auto-disabled after 21 fires in 60 seconds; check for a self-triggering watch path or an event flood",
    },
  },
];

async function installListenerIpcMock(page: Page) {
  await page.addInitScript(({ listeners }) => {
    let callbackId = 1;
    const callbacks = new Map<number, unknown>();
    let stored = listeners as Array<Record<string, unknown>>;
    const tauriWindow = window as Window & {
      __listenerInvokes?: Array<{ command: string; args?: Record<string, unknown> }>;
      __TAURI_INTERNALS__?: Record<string, unknown>;
      __TAURI_EVENT_PLUGIN_INTERNALS__?: Record<string, unknown>;
    };

    tauriWindow.__listenerInvokes = [];
    tauriWindow.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => undefined };
    tauriWindow.__TAURI_INTERNALS__ = {
      metadata: {
        currentWindow: { label: "main" },
        currentWebview: { label: "main" },
      },
      transformCallback: (callback: unknown) => {
        const id = callbackId++;
        callbacks.set(id, callback);
        return id;
      },
      unregisterCallback: (id: number) => {
        callbacks.delete(id);
      },
      convertFileSrc: (filePath: string) => filePath,
      invoke: async (command: string, args?: Record<string, unknown>) => {
        tauriWindow.__listenerInvokes?.push({ command, args });

        if (command === "list_agents") return [];
        if (command === "list_agent_classes") return [];
        if (command === "list_provider_readiness") return [];
        if (command === "load_watchlists") return [];
        if (command === "load_watchlist_prefs") return null;
        if (command === "load_agent_interactions") return {};
        if (command === "load_queue_items") return [];
        if (command === "load_queue_preferences") return {};
        if (command === "load_onboarding_hints") {
          return { dismissed_hint_ids: ["spawn-agent-first-run:v1", "automation-authoring:v1"] };
        }
        if (command === "dismiss_onboarding_hint") return { dismissed_hint_ids: [] };
        if (command === "list_automations") return [];
        if (command === "list_scheduled_runs") return [];
        if (command === "load_automation_library") return { folders: [], rootAutomationIds: [] };
        if (command === "get_library_tree") {
          return { type: "Folder", path: "", name: "Root", children: [] };
        }
        if (command === "list_deployed_skills") return [];
        if (command === "load_app_settings") return null;
        if (command === "load_shell_settings") {
          return {
            shell_id: "auto",
            custom_executable: null,
            custom_args: null,
            agent_session_persistence: "resume",
            default_provider: "codex",
          };
        }
        if (command === "list_available_shells") return [];
        if (command === "plugin:event|listen") return callbackId++;
        if (command === "plugin:event|unlisten") return null;
        if (command === "sync_provider_theme_settings") return null;

        if (command === "automation_list_blueprints") {
          return {
            blueprints: [
              { id: "code-review", name: "Code review", path: "/x/code-review.md" },
              { id: "release-notes", name: "Release notes", path: "/x/release-notes.md" },
            ],
            truncated: false,
            next_offset: null,
          };
        }
        if (command === "automation_list_runs") {
          return { runs: [], truncated: false, next_offset: null };
        }
        if (command === "automation_read_run") return { state: null, events: [], blueprint: null };
        if (command === "schedule_list") return [];

        if (command === "listener_list") return stored;
        if (command === "listener_save") {
          const listener = args?.listener as Record<string, unknown>;
          const saved = { ...listener, id: listener.id || "listener-new", has_secret: false, webhook_url: null };
          stored = [...stored, saved];
          return saved;
        }
        if (command === "listener_set_enabled") {
          stored = stored.map((listener) =>
            listener.id === args?.id ? { ...listener, enabled: args?.enabled } : listener,
          );
          return null;
        }
        if (command === "listener_delete") {
          stored = stored.filter((listener) => listener.id !== args?.id);
          return null;
        }
        if (command === "listener_set_webhook_secret") return "generated-secret-value";
        if (command === "listener_gateway_config") {
          return { schema: 1, host: "127.0.0.1", port: 8787 };
        }

        return null;
      },
    };
  }, { listeners: listenerFixtures });
}

async function openMonitor(page: Page) {
  await openAutomationEditor(page);
  await page
    .getByTestId("automations-view")
    .getByRole("button", { name: /^monitor$/i })
    .click();
}

test("listeners surface what they watch, why they stopped, and their downtime blindness", async ({
  page,
}) => {
  await mkdir(screenshotDirectory, { recursive: true });
  await installListenerIpcMock(page);
  await page.setViewportSize({ width: 1700, height: 1080 });
  await page.clock.setFixedTime(fixedBrowserTime);
  await page.goto("/", { waitUntil: "domcontentloaded" });
  await page.locator('[data-testid="app-shell"]').waitFor({ timeout: 15_000 });

  await openMonitor(page);

  const panel = page.getByTestId("automation-listeners");
  await expect(panel).toBeVisible();

  // Each variant reports what it is watching, not just that it exists.
  const fileRow = panel.getByTestId("listener-row-listener-file");
  await expect(fileRow).toContainText("Source audit");
  await expect(fileRow).toContainText("/workspace/repo (recursive) **/*.rs");
  await expect(fileRow).toContainText("Listening");
  await expect(fileRow).toContainText("12 fires");

  const hookRow = panel.getByTestId("listener-row-listener-hook");
  await expect(hookRow).toContainText("/hooks/ci");
  // A refused delivery is shown, so a webhook that "isn't firing" is
  // diagnosable rather than merely quiet.
  await expect(hookRow).toContainText("delivery credential did not match");

  const pollRow = panel.getByTestId("listener-row-listener-poll");
  await expect(pollRow).toContainText("every 15m");

  // Only the poll survives downtime; the other two say so.
  await expect(fileRow).toContainText("misses events while closed");
  await expect(hookRow).toContainText("misses events while closed");
  await expect(pollRow).not.toContainText("misses events while closed");

  // The rate ceiling reads as auto-disabled, not as a listener someone
  // switched off, because the user's own `enabled` flag was never written.
  const runawayRow = panel.getByTestId("listener-row-listener-runaway");
  await expect(runawayRow).toContainText("Auto-disabled");
  await expect(runawayRow).toContainText("self-triggering watch path");
  await expect(runawayRow.getByLabel("Disable Build output watch")).toBeVisible();

  await panel.screenshot({ path: `${screenshotDirectory}/listeners-panel.png` });
});

test("a new listener is created disabled and its trigger fields follow the chosen type", async ({
  page,
}) => {
  await mkdir(screenshotDirectory, { recursive: true });
  await installListenerIpcMock(page);
  await page.setViewportSize({ width: 1700, height: 1080 });
  await page.clock.setFixedTime(fixedBrowserTime);
  await page.goto("/", { waitUntil: "domcontentloaded" });
  await page.locator('[data-testid="app-shell"]').waitFor({ timeout: 15_000 });

  await openMonitor(page);
  await page.getByRole("button", { name: /new listener/i }).click();

  const dialog = page.getByTestId("listener-editor-dialog");
  await expect(dialog).toBeVisible();

  // File watch is the default and shows its own fields.
  await expect(dialog.getByLabel("Watch path")).toBeVisible();
  await expect(dialog.getByLabel("Quiet period (ms)")).toBeVisible();

  await dialog.getByLabel("Watch path").fill("/workspace/repo/src");
  await dialog.getByLabel("Name").fill("New source watch");
  await dialog.getByLabel("Match patterns").fill("**/*.ts");
  await dialog.screenshot({ path: `${screenshotDirectory}/listener-editor-file.png` });

  // Switching type replaces the fields rather than mixing them.
  await dialog.getByLabel("Trigger").selectOption("web_poll");
  await expect(dialog.getByLabel("URL")).toBeVisible();
  await expect(dialog.getByLabel("Watch path")).toHaveCount(0);
  await dialog.getByLabel("URL").fill("https://api.example.invalid/releases");
  await dialog.getByLabel("Fires when this changes").selectOption("json_pointer");
  await expect(dialog.getByLabel("JSON pointer")).toBeVisible();
  await dialog.getByLabel("JSON pointer").fill("/0/tag_name");
  await dialog.screenshot({ path: `${screenshotDirectory}/listener-editor-poll.png` });

  await dialog.getByRole("button", { name: /^save$/i }).click();

  const saveCall = await page.evaluate(() =>
    (window as Window & { __listenerInvokes?: Array<{ command: string; args?: Record<string, unknown> }> })
      .__listenerInvokes?.find((entry) => entry.command === "listener_save"),
  );
  expect(saveCall).toBeTruthy();
  const savedListener = saveCall?.args?.listener as Record<string, unknown>;
  // Creating a watch must not silently start spending provider tokens.
  expect(savedListener.enabled).toBe(false);
  expect((savedListener.trigger as Record<string, unknown>).type).toBe("web_poll");
  expect((savedListener.trigger as Record<string, unknown>).json_pointer).toBe("/0/tag_name");
});
