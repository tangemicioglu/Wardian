import { chromium } from "@playwright/test";
import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs/promises";
import http from "node:http";
import path from "node:path";
import process from "node:process";

const root = process.cwd();
const explicitBaseUrl = process.env.WARDIAN_DOCS_SCREENSHOT_URL;
const screenshotPort = Number.parseInt(process.env.WARDIAN_DOCS_SCREENSHOT_PORT ?? "1420", 10);
const baseUrl = explicitBaseUrl ?? `http://127.0.0.1:${screenshotPort}`;
const screenshotHome = path.join(root, ".tmp", "wardian-docs-screenshots");
const dismissedOnboardingHintIds = ["spawn-agent-first-run:v1"];
const defaultSidebarContentWidth = 240;
const wideSidebarContentWidth = 320;

const agents = [
  {
    session_id: "docs-codex",
    session_name: "Docs-Codex",
    agent_class: "Coder",
    folder: "<absolute-workspace-path>",
    provider: "codex",
    is_off: false,
    model: "gpt-5.4",
  },
  {
    session_id: "docs-reviewer",
    session_name: "Docs-Reviewer",
    agent_class: "Reviewer",
    folder: "<absolute-workspace-path>",
    provider: "claude",
    is_off: false,
    model: "opus",
  },
  {
    session_id: "docs-designer",
    session_name: "Docs-Designer",
    agent_class: "Designer",
    folder: "<absolute-workspace-path>",
    provider: "gemini",
    is_off: true,
    model: "pro",
  },
];

/**
 * The telemetry *store*, which is a different thing from the `telemetry`
 * fixture below.
 *
 * That one is live per-agent process metrics, pushed over `agent-metrics`. This
 * is what `telemetry_fleet` and `telemetry_matrix` answer — the recorded history
 * the Dashboard and Analytics are built on. Neither was mocked, so both surfaces
 * sat on "Reading the telemetry store…" forever and their captures could never
 * have succeeded even once the surface-name bug below was fixed.
 *
 * Shaped to match the three docs agents so the screenshots are internally
 * consistent: `docs-designer` runs on gemini, which publishes no token
 * accounting, and therefore reads as unmeasured rather than as zero.
 */
const TELEMETRY_BUCKETS = Array.from({ length: 24 }, (_, index) =>
  new Date(Date.UTC(2026, 4, 12, 0, index * 30)).toISOString(),
);

function docsSpark(peak, at) {
  return TELEMETRY_BUCKETS.map((_, index) =>
    index === at ? peak : Math.round(peak * (0.08 + ((index * 7) % 5) * 0.11)),
  );
}

const telemetryFleet = {
  window: { from: "2026-05-12T00:00:00.000Z", to: "2026-05-12T12:00:00.000Z", from_floored: false },
  window_minutes: 1440,
  rows: [
    {
      key: "docs-codex", label: "Docs-Codex", sublabel: "Coder",
      tokens_per_hour: 184_000, turns_per_hour: 21, active_ms: 4_520_000, turns: 63,
      total_tokens: 552_000, files_touched: 18, lines_added: 1_284, lines_removed: 396,
      tokens_reported: true, idle: false, spark: docsSpark(184_000, 17),
    },
    {
      key: "docs-reviewer", label: "Docs-Reviewer", sublabel: "Reviewer",
      tokens_per_hour: 96_500, turns_per_hour: 12, active_ms: 2_180_000, turns: 34,
      total_tokens: 289_500, files_touched: 9, lines_added: 412, lines_removed: 118,
      tokens_reported: true, idle: false, spark: docsSpark(96_500, 11),
    },
    {
      // Gemini publishes no token accounting at all.
      key: "docs-designer", label: "Docs-Designer", sublabel: "Designer",
      tokens_per_hour: null, turns_per_hour: 6, active_ms: 940_000, turns: 17,
      total_tokens: null, files_touched: 4, lines_added: 96, lines_removed: 31,
      tokens_reported: false, idle: false, spark: TELEMETRY_BUCKETS.map(() => 0),
    },
  ],
  maxima: {
    tokens_per_hour: 184_000, turns_per_hour: 21, turns: 63, active_ms: 4_520_000,
    total_tokens: 552_000, files_touched: 18, lines: 1_680, spark: 184_000,
  },
  buckets: TELEMETRY_BUCKETS,
  trend_measure: "total_tokens",
  grain: "minute15",
  habitat: {
    provider: "all", roster_agent_count: 3, active_agent_count: 3,
    active_ms: 7_640_000, turns: 114, total_tokens: 841_500, files_touched: 28,
    lines_added: 1_792, lines_removed: 545, tokens_reported: true,
    spark: docsSpark(281_000, 17), idle: false,
  },
  providers: [
    {
      provider: "codex", roster_agent_count: 1, active_agent_count: 1,
      active_ms: 4_520_000, turns: 63, total_tokens: 552_000, files_touched: 18,
      lines_added: 1_284, lines_removed: 396, tokens_reported: true,
      spark: docsSpark(184_000, 17), idle: false,
    },
    {
      provider: "claude", roster_agent_count: 1, active_agent_count: 1,
      active_ms: 2_180_000, turns: 34, total_tokens: 289_500, files_touched: 9,
      lines_added: 412, lines_removed: 118, tokens_reported: true,
      spark: docsSpark(96_500, 11), idle: false,
    },
    {
      provider: "gemini", roster_agent_count: 1, active_agent_count: 1,
      active_ms: 940_000, turns: 17, total_tokens: null, files_touched: 4,
      lines_added: 96, lines_removed: 31, tokens_reported: false,
      spark: TELEMETRY_BUCKETS.map(() => 0), idle: false,
    },
  ],
  provider_maxima: {
    tokens_per_hour: 184_000, turns_per_hour: 21, turns: 63, active_ms: 4_520_000,
    total_tokens: 552_000, files_touched: 18, lines: 1_680, spark: 184_000,
  },
};

const telemetryMatrix = {
  dimension: "agent",
  measure: "active_ms",
  grain: "hour",
  window: telemetryFleet.window,
  buckets: TELEMETRY_BUCKETS,
  rows: [
    { key: "docs-codex", label: "Docs-Codex", sublabel: "Coder", cells: docsSpark(3_600_000, 17), total: 4_520_000 },
    { key: "docs-reviewer", label: "Docs-Reviewer", sublabel: "Reviewer", cells: docsSpark(1_800_000, 11), total: 2_180_000 },
    { key: "docs-designer", label: "Docs-Designer", sublabel: "Designer", cells: docsSpark(900_000, 6), total: 940_000 },
  ],
  max_cell: 3_600_000,
  cells_are_not_additive: false,
};

const agentClasses = [
  { name: "Coder", description: "Implementation and verification work", is_default: true },
  { name: "Reviewer", description: "Patch review and risk analysis", is_default: true },
  { name: "Designer", description: "Interface critique and visual polish", is_default: true },
];

const telemetry = [
  {
    session_id: "docs-codex",
    cpu_usage: 14.2,
    memory_mb: 412,
    uptime_seconds: 842,
    query_count: 7,
    init_timestamp: "2026-05-12T10:05:00.000Z",
    current_status: "Processing...",
    log_path: null,
  },
  {
    session_id: "docs-reviewer",
    cpu_usage: 1.8,
    memory_mb: 226,
    uptime_seconds: 1260,
    query_count: 3,
    init_timestamp: "2026-05-12T09:58:00.000Z",
    current_status: "Idle",
    log_path: null,
  },
  {
    session_id: "docs-designer",
    cpu_usage: 0.4,
    memory_mb: 198,
    uptime_seconds: 620,
    query_count: 2,
    init_timestamp: "2026-05-12T10:12:00.000Z",
    current_status: "Off",
    log_path: null,
  },
];

const workbenchDocument = {
  schema_version: 1,
  revision: 1,
  saved_at: "2026-05-12T10:20:00.000Z",
  root: { kind: "group", group_id: "docs-group" },
  groups: {
    "docs-group": {
      group_id: "docs-group",
      surface_ids: ["docs-overview"],
      active_surface_id: "docs-overview",
    },
  },
  surfaces: {
    "docs-overview": {
      surface_id: "docs-overview",
      surface_type: "agents-overview",
      state_schema_version: 1,
      state: {
        mode: "auto",
        focused_agent_id: "docs-codex",
        search_query: "",
        status_filter: [],
      },
    },
  },
  active_group_id: "docs-group",
  recently_closed: [],
  shell: {
    left_sidebar_collapsed: false,
    left_sidebar_width: 240,
    right_sidebar_collapsed: false,
    right_sidebar_width: 240,
    bottom_terminal_open: false,
    bottom_terminal_height: 360,
  },
};

const terminalOutput = {
  "docs-codex":
    "\x1b]0;Working\x07$ Summarize this workspace in five bullets. Do not edit files.\n" +
    "- docs/ contains the public guide and developer documentation.\n" +
    "- src/ contains the React desktop Habitat UI.\n" +
    "- src-tauri/ contains the native runtime and provider orchestration.\n" +
    "- scripts/ contains automation for repeatable docs screenshots.\n" +
    "- Queue will keep this completed summary available for triage.\n",
  "docs-reviewer": "\x1b]0;Ready\x07Review complete. No blocking findings.\n",
  "docs-designer": "\x1b]0;Action Required\x07Approval needed before replacing the current hero capture.\n",
};

const terminalLinkOutput = {
  ...terminalOutput,
  "docs-codex":
    "\x1b]0;Working\x07$ wardian terminal-link smoke\r\n" +
    "URL: https://wardian.dev\r\n" +
    "File: src/App.tsx:12\r\n" +
    "Ignored command: /model\r\n" +
    "Ignored heading: stage/reason/risk\r\n",
};

const repoRoot = "<absolute-workspace-path>";

const directoryTree = {
  [repoRoot]: [
    { name: "docs", path: `${repoRoot}/docs`, is_dir: true, extension: null },
    { name: "src", path: `${repoRoot}/src`, is_dir: true, extension: null },
    { name: "package.json", path: `${repoRoot}/package.json`, is_dir: false, extension: "json" },
    { name: "README.md", path: `${repoRoot}/README.md`, is_dir: false, extension: "md" },
  ],
  [`${repoRoot}/docs`]: [
    { name: "guide", path: `${repoRoot}/docs/guide`, is_dir: true, extension: null },
    { name: "developer", path: `${repoRoot}/docs/developer`, is_dir: true, extension: null },
    { name: "index.md", path: `${repoRoot}/docs/index.md`, is_dir: false, extension: "md" },
  ],
  [`${repoRoot}/docs/guide`]: [
    { name: "ui-overview.md", path: `${repoRoot}/docs/guide/ui-overview.md`, is_dir: false, extension: "md" },
    { name: "source-control.md", path: `${repoRoot}/docs/guide/source-control.md`, is_dir: false, extension: "md" },
    { name: "automations.md", path: `${repoRoot}/docs/guide/automations.md`, is_dir: false, extension: "md" },
  ],
  [`${repoRoot}/docs/developer`]: [
    { name: "screenshot-documentation.md", path: `${repoRoot}/docs/developer/screenshot-documentation.md`, is_dir: false, extension: "md" },
  ],
  [`${repoRoot}/src`]: [
    { name: "views", path: `${repoRoot}/src/views`, is_dir: true, extension: null },
    { name: "features", path: `${repoRoot}/src/features`, is_dir: true, extension: null },
  ],
};

const gitStatus = {
  branch: "docs/task-oriented-feature-guides",
  ahead: 1,
  behind: 0,
  files: [
    { path: "docs/guide/ui-overview.md", status: "M", is_staged: true },
    { path: "docs/developer/screenshot-documentation.md", status: "M", is_staged: false },
    { path: "docs/assets/screenshots/grid/app-shell.png", status: "?", is_staged: false },
  ],
};

const gitHistory = [
  {
    hash: "8f6d1c9b4a7e2d01",
    message: "docs: add screenshot documentation plan",
    author: "Wardian",
    date: "2026-05-12 10:22:00 -0400",
  },
  {
    hash: "61a4d2c9a017bb52",
    message: "fix: stabilize source control loading state",
    author: "Wardian",
    date: "2026-05-12 09:41:00 -0400",
  },
];

const libraryTree = {
  type: "Folder",
  path: "",
  name: "Root",
  children: [
    {
      type: "Folder",
      path: "feature-prompts",
      name: "feature-prompts",
      children: [],
    },
    {
      type: "Prompt",
      path: "review/checklist.md",
      name: "Review Checklist",
      content: "Review the current branch and return findings first.",
      metadata: {
        id: "prompt-review-checklist",
        tags: ["review", "quality"],
        is_starred: true,
      },
    },
    {
      type: "Prompt",
      path: "automation/plan.md",
      name: "Automation Plan",
      content: "Break this task into bounded agent steps.",
      metadata: {
        id: "prompt-automation-plan",
        tags: ["automation"],
        is_starred: false,
      },
    },
  ],
};

const emptyLibraryTree = { path: "", name: "Root", children: [] };
const libraryIndex = {
  sections: {
    skills: { stubbed: false, tree: emptyLibraryTree },
    prompts: {
      stubbed: false,
      tree: {
        path: "",
        name: "Root",
        children: [
          {
            kind: "prompt",
            name: "Review Checklist",
            path: "review/checklist.md",
            entry_ref: "prompts/review/checklist.md",
            description: "Focused patch review with findings first.",
            tags: ["review", "quality"],
            is_starred: true,
            deployment_count: 0,
            error: null,
          },
        ],
      },
    },
    automations: { stubbed: false, tree: emptyLibraryTree },
    classes: { stubbed: false, tree: emptyLibraryTree },
    mcps: { stubbed: true, tree: emptyLibraryTree },
  },
  deployments: {},
  orphans: [],
};

const automations = [
  {
    id: "docs-automation",
    name: "Docs Screenshot Refresh",
    settings: { max_iterations: 3, on_limit_reached: "pause" },
    nodes: [
      {
        id: "trigger-1",
        type: "trigger",
        name: "Manual Trigger",
        config: { type: "manual" },
        position: { x: 120, y: 160 },
      },
      {
        id: "agent-1",
        type: "agent",
        name: "Agent Task",
        config: {
          agent_class: "Coder",
          prompt: "Capture and verify the next documentation screenshot.",
        },
        dependencies: [{ node_id: "trigger-1", port: "default" }],
        position: { x: 420, y: 160 },
      },
    ],
  },
];

const queueItems = [
  {
    id: "docs-first-run-result",
    type: "agent_completed",
    timestamp: 1778590740000,
    read: false,
    agent_session_id: "docs-codex",
    agent_name: "Docs-Codex",
    summary:
      "Completed the first read-only workspace pass. The agent identified the guide, docs, and source folders and suggested reviewing Queue before assigning follow-up edits.\n\nNext steps:\n- Open the source-control panel and inspect the pending documentation changes.\n- Compare the regenerated screenshots against the guides that reference them.\n- Ask a reviewer agent for a focused pass before committing the branch.",
  },
  {
    id: "docs-automation-completion",
    type: "automation_completed",
    timestamp: 1778585100000,
    read: true,
    automation_id: "docs-automation",
    automation_run_id: "docs-run-1",
    automation_name: "Docs Screenshot Refresh",
    status: "completed",
    summary: "Captured feature screenshots for the guide refresh.",
  },
];

const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function isUrlReady(url = baseUrl) {
  return new Promise((resolve) => {
    const req = http.get(url, (res) => {
      res.resume();
      resolve(res.statusCode && res.statusCode < 500);
    });
    req.on("error", () => resolve(false));
    req.setTimeout(1_000, () => {
      req.destroy();
      resolve(false);
    });
  });
}

async function waitForServer() {
  for (let i = 0; i < 90; i += 1) {
    if (await isUrlReady()) return;
    await wait(1_000);
  }
  throw new Error(`Timed out waiting for ${baseUrl}`);
}

async function startOwnedServer() {
  if (await isUrlReady()) {
    throw new Error(
      `${baseUrl} is already serving content. Stop that process, set WARDIAN_DOCS_SCREENSHOT_PORT, or set WARDIAN_DOCS_SCREENSHOT_URL to opt into capturing an existing app.`,
    );
  }

  const child = spawn(`npm run vite -- --host 127.0.0.1 --port ${screenshotPort} --strictPort`, {
    cwd: root,
    env: {
      ...process.env,
      WARDIAN_HOME: screenshotHome,
    },
    shell: true,
    stdio: "inherit",
  });
  return child;
}

function stopOwnedServer(child) {
  if (!child?.pid) return;
  if (process.platform === "win32") {
    spawnSync("taskkill", ["/PID", String(child.pid), "/T", "/F"], { stdio: "ignore" });
    return;
  }
  child.kill();
}

async function stabilizeVisuals(page) {
  await page.addStyleTag({
    content: `
      *, *::before, *::after {
        animation-delay: 0s !important;
        animation-duration: 0s !important;
        caret-color: transparent !important;
        transition-delay: 0s !important;
        transition-duration: 0s !important;
      }
    `,
  });
}

async function ensureDir(filePath) {
  await fs.mkdir(path.dirname(filePath), { recursive: true });
}

/**
 * Every capture this run is expected to produce, in the order it produces them.
 *
 * The list exists so a failure can say what it *skipped*. This script is a long
 * linear sequence, and a step that times out halfway through silently abandons
 * everything after it — which is how the Dashboard screenshot stayed stale for
 * releases without anyone noticing. Failing loudly is not enough on its own;
 * the operator has to be told which images are now untrustworthy.
 */
const EXPECTED_CAPTURES = [
  "terminal/clickable-links.png",
  "grid/app-shell.png",
  "grid/active-agent-state.png",
  "workbench-navigation/command-palette.png",
  "watchlists/agent-roster.png",
  "spawn-agent/spawn-form.png",
  "command-panel/broadcast-prompt.png",
  "settings/runtime-settings.png",
  "explorer/workspace-tree.png",
  "source-control/status-panel.png",
  "queue/queue-view.png",
  "queue/completed-result.png",
  "library/library-view.png",
  "automations/builder-canvas.png",
  "dashboard/system-summary.png",
  "analytics/activity-matrix.png",
];

const captured = new Set();

/** What `EXPECTED_CAPTURES` lists that this run never produced. */
function missedCaptures() {
  return EXPECTED_CAPTURES.filter((relativePath) => !captured.has(relativePath));
}

async function capture(page, relativePath, locator) {
  if (!EXPECTED_CAPTURES.includes(relativePath)) {
    // Keeps the manifest honest: a capture added to the sequence and not to the
    // list would never be reported as skipped.
    throw new Error(`${relativePath} is captured but missing from EXPECTED_CAPTURES`);
  }
  if (await page.getByText("Fatal UI Rendering Error").isVisible().catch(() => false)) {
    throw new Error(`Refusing to capture ${relativePath}: app is showing the error boundary`);
  }

  const filePath = path.join(root, "docs", "assets", "screenshots", relativePath);
  await ensureDir(filePath);
  if (locator) {
    await locator.screenshot({ path: filePath, animations: "disabled" });
  } else {
    await assertShellHasNoHorizontalOverlap(page, relativePath);
    await page.screenshot({ path: filePath, animations: "disabled" });
  }
  captured.add(relativePath);
  console.log(`captured ${path.relative(root, filePath)}`);
}

async function setSidebarContentWidth(page, width) {
  await page.evaluate((nextWidth) => {
    document.documentElement.style.setProperty("--sidebar-content-width", `${nextWidth}px`);
  }, width);
}

async function openWorkbenchSurface(page, surfaceType) {
  await page.keyboard.press(process.platform === "darwin" ? "Meta+P" : "Control+P");
  const dialog = page.getByRole("dialog", { name: "Open Surface" });
  await dialog.waitFor({ timeout: 10_000 });
  await dialog.locator(`[role="option"][data-surface-type="${surfaceType}"]`).click();
  await page.locator(`[role="tab"][data-surface-type="${surfaceType}"][aria-selected="true"]`)
    .waitFor({ timeout: 10_000 });
}

async function assertShellHasNoHorizontalOverlap(page, relativePath) {
  const rects = await page.evaluate(() => {
    const rectFor = (selector) => {
      const element = document.querySelector(selector);
      if (!element) return null;
      const rect = element.getBoundingClientRect();
      return {
        left: rect.left,
        right: rect.right,
        width: rect.width,
      };
    };

    return {
      main: rectFor("main"),
      roster: rectFor('[data-testid="agent-watchlist"]'),
      grid: rectFor('[data-testid="agent-grid"]'),
      sidebarWidth: getComputedStyle(document.documentElement).getPropertyValue("--sidebar-content-width").trim(),
    };
  });

  if (!rects.main || !rects.roster) return;

  if (rects.main.right > rects.roster.left + 1) {
    throw new Error(
      [
        `Refusing to capture ${relativePath}: main pane overlaps the right roster.`,
        `main.right=${rects.main.right}`,
        `roster.left=${rects.roster.left}`,
        `sidebar-content-width=${rects.sidebarWidth}`,
      ].join(" "),
    );
  }

  if (rects.grid && rects.grid.right > rects.roster.left + 1) {
    throw new Error(
      [
        `Refusing to capture ${relativePath}: Grid extends under the right roster.`,
        `grid.right=${rects.grid.right}`,
        `roster.left=${rects.roster.left}`,
        `grid.width=${rects.grid.width}`,
        `sidebar-content-width=${rects.sidebarWidth}`,
      ].join(" "),
    );
  }
}

async function installTauriDocsMock(page, options = {}) {
  const effectiveTerminalOutput = options.terminalOutput ?? terminalOutput;
  await page.addInitScript(({ agents, agentClasses, telemetry, telemetryFleet, telemetryMatrix, terminalOutput, libraryTree, libraryIndex, automations, queueItems, repoRoot, directoryTree, gitStatus, gitHistory, dismissedOnboardingHintIds, workbenchDocument }) => {
    const fixedNow = 1778590800000;
    const RealDate = Date;

    window.localStorage.removeItem("wardian-layout");

    class FixedDate extends RealDate {
      constructor(...args) {
        super(...(args.length === 0 ? [fixedNow] : args));
      }

      static now() {
        return fixedNow;
      }
    }

    window.Date = FixedDate;

    const callbacks = new Map();
    const listeners = new Map();
    const terminalReads = {};
    let workbenchState = structuredClone(workbenchDocument);
    let callbackId = 1;

    const tauriWindow = window;
    tauriWindow.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
      unregisterListener: () => undefined,
    };

    const registerListener = (eventName, callback) => {
      const list = listeners.get(eventName) || [];
      list.push(callback);
      listeners.set(eventName, list);
    };

    tauriWindow.__WARDIAN_DOCS_EMIT = (eventName, payload) => {
      for (const callback of listeners.get(eventName) || []) {
        callback({ event: eventName, payload });
      }
    };

    tauriWindow.__TAURI_INTERNALS__ = {
      metadata: {
        currentWindow: { label: "main" },
        currentWebview: { label: "main" },
      },
      transformCallback: (callback) => {
        const id = callbackId++;
        callbacks.set(id, callback);
        return id;
      },
      unregisterCallback: (id) => {
        callbacks.delete(id);
      },
      convertFileSrc: (filePath) => filePath,
      invoke: async (command, args = {}) => {
        if (command === "plugin:event|listen") {
          const handler = callbacks.get(args.handler);
          if (args.event && handler) registerListener(args.event, handler);
          return callbackId++;
        }
        if (command === "plugin:event|unlisten") return null;
        if (command === "get_workbench_boot_config") return { safe_mode: false };
        if (command === "load_workbench_state") {
          return {
            source: "primary",
            document: structuredClone(workbenchState),
            notice: null,
            durable_revision: workbenchState.revision,
            durable_token: `docs-${workbenchState.revision}`,
          };
        }
        if (command === "save_workbench_state") {
          workbenchState = structuredClone(args.document);
          return {
            outcome: "saved",
            durable_revision: workbenchState.revision,
            durable_token: `docs-${workbenchState.revision}`,
            request_id: args.request_id,
          };
        }
        if (command === "list_agents") return agents;
        if (command === "list_agent_classes") return agentClasses;
        if (command === "load_watchlists") {
          return {
            version: 2,
            watchlists: [
              {
                id: "docs",
                name: "Docs",
                entries: agents.map((agent) => ({ type: "agent", agentId: agent.session_id })),
                agentIds: agents.map((agent) => agent.session_id),
              },
            ],
            teams: [
              {
                id: "team-docs",
                name: "Docs Team",
                agentIds: ["docs-codex", "docs-reviewer"],
              },
            ],
          };
        }
        if (command === "load_watchlist_prefs") return null;
        if (command === "load_agent_interactions") {
          return {
            "docs-codex": "2026-05-12T10:18:00.000Z",
            "docs-reviewer": "2026-05-12T10:16:00.000Z",
          };
        }
        if (command === "load_queue_items") return queueItems;
        // The telemetry store. Without these the Dashboard and Analytics never
        // leave their loading state, so their captures cannot succeed.
        if (command === "telemetry_fleet") return telemetryFleet;
        if (command === "telemetry_matrix") return telemetryMatrix;
        if (command === "telemetry_refresh") return { sources: 3, advanced: 0, turns: 0, edits: 0, intervals: 0, buckets_recomputed: 0, unavailable: 0, failures: [] };
        if (command === "load_dashboard_prefs") return null;
        if (command === "save_dashboard_prefs") return null;
        if (command === "get_explorer_root") return repoRoot;
        if (command === "get_directory_tree") return directoryTree[args.path] || [];
        if (command === "read_file_preview") {
          return `# ${String(args.path || "").split("/").pop()}\n\nDocumentation preview content for the seeded screenshot workspace.\n`;
        }
        if (command === "git_status") return gitStatus;
        if (command === "git_log") return gitHistory;
        if (command === "git_diff_file") {
          return [
            "diff --git a/docs/guide/ui-overview.md b/docs/guide/ui-overview.md",
            "--- a/docs/guide/ui-overview.md",
            "+++ b/docs/guide/ui-overview.md",
            "@@ -1,3 +1,6 @@",
            " # UI Overview",
            "+",
            "+![Wardian grid](../assets/screenshots/grid/app-shell.png)",
          ].join("\n");
        }
        if (command === "git_watch" || command === "git_unwatch" || command === "list_agent_worktrees") return [];
        if (command === "load_shell_settings") {
          return {
            shell_id: "auto",
            custom_executable: null,
            custom_args: null,
            agent_session_persistence: "resume",
          };
        }
        if (command === "list_available_shells") {
          return [
            { id: "pwsh", label: "PowerShell 7", executable: "pwsh" },
            { id: "powershell", label: "Windows PowerShell", executable: "powershell.exe" },
            { id: "cmd", label: "Command Prompt", executable: "cmd.exe" },
          ];
        }
        if (command === "save_shell_settings" || command === "save_agent_session_persistence") {
          return {
            shell_id: "auto",
            custom_executable: null,
            custom_args: null,
            agent_session_persistence: "resume",
          };
        }
        if (command === "load_onboarding_hints") {
          return { dismissed_hint_ids: dismissedOnboardingHintIds };
        }
        if (command === "dismiss_onboarding_hint") {
          return {
            dismissed_hint_ids: Array.from(new Set([...dismissedOnboardingHintIds, args.hintId])).sort(),
          };
        }
        if (command === "list_automations") return automations;
        if (command === "automation_list_runs") return [];
        if (command === "schedule_list") return [];
        if (command === "automation_list_blueprints") {
          return automations.map((automation) => ({
            id: automation.id,
            name: automation.name,
            path: `${repoRoot}/library/automations/${automation.id}.md`,
          }));
        }
        if (command === "automation_parse") {
          const automation = automations[0];
          return {
            blueprint: {
              schema: 2,
              id: automation.id,
              name: automation.name,
              nodes: automation.nodes,
              edges: [],
            },
            diagnostics: [],
          };
        }
        if (command === "automation_validate") return { ok: true, diagnostics: [] };
        if (command === "load_automation_library") {
          return {
            folders: [
              {
                id: "folder-docs",
                name: "Documentation",
                automationIds: ["docs-automation"],
                isCollapsed: false,
              },
            ],
            rootAutomationIds: [],
          };
        }
        if (command === "save_automation_library") return null;
        if (command === "list_scheduled_runs") return [];
        if (command === "get_library_tree") return libraryTree;
        if (command === "get_library_index") return libraryIndex;
        if (command === "read_library_item") {
          return "Review the current branch and return findings first.";
        }
        if (command === "library_watch" || command === "library_unwatch") return null;
        if (command === "list_deployed_skills" || command === "list_deployed_skill_refs") return [];
        if (command === "sync_provider_theme_settings") return null;
        if (command === "read_agent_pty") {
          const sessionId = args.sessionId;
          if (!sessionId || terminalReads[sessionId]) return null;
          terminalReads[sessionId] = true;
          return terminalOutput[sessionId] || null;
        }
        if (command === "terminal_link_target_exists") {
          const target = String(args.path ?? "").replace(/\\/g, "/");
          return target.endsWith("/src/App.tsx");
        }
        if (
          command === "resize_agent_terminal" ||
          command === "send_input_to_agent" ||
          command === "send_binary_input_to_agent" ||
          command === "submit_prompt_to_agent" ||
          command === "submit_prompt_to_agents" ||
          command === "save_watchlists" ||
          command === "save_watchlist_prefs" ||
          command === "save_agent_interactions" ||
          command === "open_library_folder"
        ) {
          return null;
        }
        return null;
      },
    };

    setTimeout(() => {
      tauriWindow.__WARDIAN_DOCS_EMIT("agent-metrics", telemetry);
      tauriWindow.__WARDIAN_DOCS_EMIT("app-metrics", { cpu_usage: 18.4, memory_mb: 1224 });
      tauriWindow.__WARDIAN_DOCS_EMIT("agent-json-event", {
        session_id: "docs-codex",
        data: { type: "progress", content: "Capturing screenshots" },
      });
    }, 600);
  }, { agents, agentClasses, telemetry, telemetryFleet, telemetryMatrix, terminalOutput: effectiveTerminalOutput, libraryTree, libraryIndex, automations, queueItems, repoRoot, directoryTree, gitStatus, gitHistory, dismissedOnboardingHintIds, workbenchDocument });
}

function collectPageDiagnostics(page, browserErrors) {
  page.on("pageerror", (error) => {
    browserErrors.push(`page error: ${error.stack || error.message}`);
  });
  page.on("console", (message) => {
    if (message.type() === "error") {
      browserErrors.push(`browser console: ${message.text()}`);
    }
  });
}

async function openDocsPage(browser, browserErrors, mockOptions = {}) {
  const page = await browser.newPage({ viewport: { width: 1680, height: 960 }, deviceScaleFactor: 1 });
  collectPageDiagnostics(page, browserErrors);
  await installTauriDocsMock(page, mockOptions);
  await page.goto(baseUrl, { waitUntil: "domcontentloaded" });
  await page.locator('[data-testid="app-shell"]').waitFor({ timeout: 15_000 });
  await stabilizeVisuals(page);
  await page.waitForTimeout(1_500);
  return page;
}

async function captureTerminalLinkEvidence(browser, browserErrors) {
  const page = await openDocsPage(browser, browserErrors, { terminalOutput: terminalLinkOutput });
  try {
    await page.locator('[data-testid="agent-grid"]').waitFor({ timeout: 10_000 });
    await page.locator('#agent-card-docs-codex [data-testid="agent-terminal-host"]').waitFor({ timeout: 10_000 });
    await page.waitForTimeout(500);
    await page.locator('#agent-card-docs-codex [data-testid="agent-terminal-host"]').hover({ position: { x: 110, y: 42 } });
    await page.waitForTimeout(300);
    await capture(page, "terminal/clickable-links.png", page.locator("#agent-card-docs-codex"));
  } finally {
    await page.close();
  }
}

async function main() {
  await fs.mkdir(screenshotHome, { recursive: true });

  let server = null;
  if (explicitBaseUrl) {
    await waitForServer();
  } else {
    server = await startOwnedServer();
    await waitForServer();
  }

  const browser = await chromium.launch();
  const browserErrors = [];

  try {
    await captureTerminalLinkEvidence(browser, browserErrors);

    const page = await openDocsPage(browser, browserErrors);

    await page.locator('[data-testid="agent-grid"]').waitFor({ timeout: 10_000 });
    await capture(page, "grid/app-shell.png");
    await capture(page, "grid/active-agent-state.png", page.locator("main"));

    await page.keyboard.press("Control+Shift+P");
    await page.getByRole("dialog", { name: "Command Palette" }).waitFor({ timeout: 10_000 });
    await page.waitForTimeout(300);
    await capture(page, "workbench-navigation/command-palette.png");
    await page.keyboard.press("Escape");
    await page.getByRole("dialog", { name: "Command Palette" }).waitFor({
      state: "hidden",
      timeout: 10_000,
    });

    await page.locator('[data-testid="agent-watchlist"]').waitFor({ timeout: 10_000 });
    await capture(page, "watchlists/agent-roster.png", page.locator('[data-testid="agent-watchlist"]'));

    await setSidebarContentWidth(page, wideSidebarContentWidth);

    await page.locator('[data-testid="sidebar-tab-agent-config"]').click();
    await page.waitForTimeout(500);
    await page.locator('[data-testid="spawn-agent-name"]').fill("docs-demo");
    await page.locator('[data-testid="spawn-workspace-path"]').fill("<absolute-workspace-path>");
    await page.locator('[data-testid="spawn-agent-name"]').waitFor({ timeout: 10_000 });
    await page.locator('[data-testid="spawn-workspace-path"]').blur();
    await capture(page, "spawn-agent/spawn-form.png");

    await page.locator('[data-testid="sidebar-tab-command"]').click();
    await page.waitForTimeout(500);
    await page.locator('[data-testid="broadcast-textarea"]').fill("Summarize this workspace in five bullets. Do not edit files.");
    await page.locator('[data-testid="broadcast-textarea"]').waitFor({ timeout: 10_000 });
    await page.locator('[data-testid="broadcast-textarea"]').blur();
    await capture(page, "command-panel/broadcast-prompt.png");

    await page.locator('[data-testid="sidebar-tab-settings"]').click();
    await page.getByRole("button", { name: "Agent Runtime" }).click();
    await page.getByRole("heading", { name: "Agent Runtime" }).waitFor({ timeout: 10_000 });
    await page.waitForTimeout(500);
    await capture(page, "settings/runtime-settings.png", page.getByRole("dialog", { name: "Settings" }));
    await page.getByRole("button", { name: "Close settings" }).click();
    await page.getByRole("dialog", { name: "Settings" }).waitFor({ state: "hidden", timeout: 10_000 });

    await setSidebarContentWidth(page, defaultSidebarContentWidth);

    await openWorkbenchSurface(page, "agents-overview");
    await page.locator("#agent-card-docs-codex").click();
    await page.locator('[data-testid="sidebar-tab-explorer"]').click();
    await page.waitForTimeout(700);
    await page.getByText("docs", { exact: true }).click();
    await page.waitForTimeout(300);
    await page.getByText("guide", { exact: true }).click();
    await page.getByText("ui-overview.md").waitFor({ timeout: 10_000 });
    await page.waitForTimeout(300);
    await capture(page, "explorer/workspace-tree.png", page.locator('[data-testid="explorer-panel"]'));

    await page.locator('[data-testid="sidebar-tab-git"]').click();
    await page.getByRole("heading", { name: "Source Control", exact: true }).waitFor({ timeout: 10_000 });
    await page.waitForTimeout(700);
    await capture(page, "source-control/status-panel.png", page.locator("aside").filter({ hasText: "Source Control" }).first());

    // `inbox`, not `queue`. The surface was renamed and this call was not,
    // so every capture below it silently stopped running. The image paths keep
    // the `queue/` prefix because the guides link to them by that name.
    await openWorkbenchSurface(page, "inbox");
    await page.getByText("Automation completed").waitFor({ timeout: 10_000 });
    await page.waitForTimeout(700);
    await capture(page, "queue/queue-view.png", page.locator("main"));
    await page.getByTestId("queue-item-summary-docs-first-run-result").waitFor({ timeout: 10_000 });
    await page.getByRole("button", { name: "Show full summary" }).click();
    await page.waitForTimeout(700);
    await capture(page, "queue/completed-result.png", page.locator("main"));

    await openWorkbenchSurface(page, "library");
    await page.getByTestId("library-section-prompts").click();
    await page.getByTestId("library-row-prompts/review/checklist.md").click();
    await page.getByRole("heading", { name: "Review Checklist" }).waitFor({ timeout: 10_000 });
    await page.waitForTimeout(700);
    await capture(page, "library/library-view.png");

    await openWorkbenchSurface(page, "automations");
    await page.getByTestId("automations-view").waitFor({ timeout: 10_000 });
    await page.waitForTimeout(700);
    await capture(page, "automations/builder-canvas.png");

    await openWorkbenchSurface(page, "dashboard");
    // The fleet table, not a per-agent card: the Dashboard is one row per
    // agent now, so `#agent-card-*` no longer exists on this surface.
    await page.locator(
      '[data-testid="surface-panel"][data-surface-type="dashboard"] .dashboard-view__table',
    ).waitFor({ timeout: 10_000 });
    await page.waitForTimeout(700);
    await capture(page, "dashboard/system-summary.png");

    await openWorkbenchSurface(page, "analytics");
    await page.locator(
      '[data-testid="surface-panel"][data-surface-type="analytics"] .analytics-view__matrix',
    ).waitFor({ timeout: 10_000 });
    await page.waitForTimeout(700);
    await capture(page, "analytics/activity-matrix.png");

    if (browserErrors.length > 0) {
      throw new Error(`Browser errors were logged during screenshot capture:\n${browserErrors.join("\n")}`);
    }
  } finally {
    await browser.close();
    if (server) {
      stopOwnedServer(server);
    }
  }
}

/** One capture path per line, indented, for a console list. */
function listCaptures(paths) {
  return paths.map((relativePath) => `  ${relativePath}`).join("\n");
}

main()
  .then(() => {
    const missed = missedCaptures();
    if (missed.length > 0) {
      // Reachable only if the sequence returns without throwing while still
      // skipping something — a conditional step, or a manifest that drifted.
      console.error(
        `\nRun finished but ${missed.length} capture(s) never ran:\n${listCaptures(missed)}`,
      );
      process.exit(1);
    }
  })
  .catch((error) => {
    console.error(error);
    const missed = missedCaptures();
    if (missed.length > 0) {
      // The point of naming them: these files are still on disk from an earlier
      // run, so they look current and are not. That is how the Dashboard
      // screenshot stayed stale for releases.
      console.error(
        `\n${missed.length} capture(s) were never reached and are now stale on disk:\n${listCaptures(missed)}`,
      );
    }
    process.exit(1);
  });
