/**
 * Shared fixtures and the in-browser Tauri mock for documentation capture.
 *
 * Both the still-screenshot capture (`capture-doc-screenshots.mjs`) and the
 * site media capture (`capture-site-media.mjs`) drive the same seeded app, so
 * the fixtures live here once. Duplicating them would let the two captures
 * drift apart and show different data for the same surface.
 *
 * Everything here is deterministic and public-safe: workspace paths are the
 * `<absolute-workspace-path>` placeholder, and the agents are invented.
 */

export const dismissedOnboardingHintIds = ["spawn-agent-first-run:v1"];

export const agents = [
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
export const TELEMETRY_BUCKETS = Array.from({ length: 24 }, (_, index) =>
  new Date(Date.UTC(2026, 4, 12, 0, index * 30)).toISOString(),
);

export function docsSpark(peak, at) {
  return TELEMETRY_BUCKETS.map((_, index) =>
    index === at ? peak : Math.round(peak * (0.08 + ((index * 7) % 5) * 0.11)),
  );
}

export const telemetryFleet = {
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

export const telemetryMatrix = {
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

export const agentClasses = [
  { name: "Coder", description: "Implementation and verification work", is_default: true },
  { name: "Reviewer", description: "Patch review and risk analysis", is_default: true },
  { name: "Designer", description: "Interface critique and visual polish", is_default: true },
];

/**
 * Model options offered by the spawn panel's provider dropdown.
 *
 * The panel is on screen in most captures, so an unanswered
 * `list_provider_model_catalog` is not a silent gap — it renders an amber
 * "Provider returned an invalid model catalogue." right where the camera is
 * pointing.
 */
export const providerModelCatalog = {
  claude: [
    { id: "opus", display_name: "Opus", effort_options: ["high", "xhigh"], default_effort: "high", is_default: true },
    { id: "sonnet", display_name: "Sonnet", effort_options: ["high", "xhigh"], default_effort: "high", is_default: false },
  ],
  codex: [
    { id: "gpt-5.6-sol", display_name: "GPT-5.6 Sol", effort_options: ["high", "xhigh"], default_effort: "high", is_default: true },
    { id: "gpt-5.6-luna", display_name: "GPT-5.6 Luna", effort_options: ["high", "xhigh"], default_effort: "xhigh", is_default: false },
  ],
};

/** Name the spawn panel pre-fills per class, so it never sits on "Loading generated name...". */
export const generatedAgentNames = {
  Coder: "Docs-Coder-2",
  Reviewer: "Docs-Reviewer-2",
  Designer: "Docs-Designer-2",
};

export const providerReadiness = [
  { provider: "claude", display_name: "Claude Code", available: true, executable: "claude", reason: null },
  { provider: "codex", display_name: "Codex", available: true, executable: "codex", reason: null },
];

export const telemetry = [
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

export const workbenchDocument = {
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

// Every newline here is CRLF. A terminal treats a bare "\n" as line feed only:
// the cursor drops a row without returning to column 0, so each line starts
// where the previous one ended and the output staircases off the right edge.
// `terminalLinkOutput` below always used "\r\n"; this block did not, which is
// why captured terminals rendered as scrambled fragments.
export const terminalOutput = {
  "docs-codex":
    "\x1b]0;Working\x07$ Summarize this workspace in five bullets. Do not edit files.\r\n" +
    "- docs/ contains the public guide and developer documentation.\r\n" +
    "- src/ contains the React desktop Habitat UI.\r\n" +
    "- src-tauri/ contains the native runtime and provider orchestration.\r\n" +
    "- scripts/ contains automation for repeatable docs screenshots.\r\n" +
    "- Queue will keep this completed summary available for triage.\r\n",
  "docs-reviewer": "\x1b]0;Ready\x07Review complete. No blocking findings.\r\n",
  "docs-designer": "\x1b]0;Action Required\x07Approval needed before replacing the current hero capture.\r\n",
};

export const terminalLinkOutput = {
  ...terminalOutput,
  "docs-codex":
    "\x1b]0;Working\x07$ wardian terminal-link smoke\r\n" +
    "URL: https://wardian.dev\r\n" +
    "File: src/App.tsx:12\r\n" +
    "Ignored command: /model\r\n" +
    "Ignored heading: stage/reason/risk\r\n",
};

export const repoRoot = "<absolute-workspace-path>";

export const directoryTree = {
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

export const gitStatus = {
  branch: "docs/task-oriented-feature-guides",
  ahead: 1,
  behind: 0,
  files: [
    { path: "docs/guide/ui-overview.md", status: "M", is_staged: true },
    { path: "docs/developer/screenshot-documentation.md", status: "M", is_staged: false },
    { path: "docs/assets/screenshots/grid/app-shell.png", status: "?", is_staged: false },
  ],
};

export const gitHistory = [
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

export const libraryTree = {
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

export const emptyLibraryTree = { path: "", name: "Root", children: [] };
export const libraryIndex = {
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

export const automations = [
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

export const queueItems = [
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

export async function stabilizeVisuals(page) {
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

/**
 * Install the seeded Tauri IPC mock on a page.
 *
 * @param {import("@playwright/test").Page} page
 * @param {object} [options]
 * @param {Record<string, string>} [options.terminalOutput] Per-agent PTY text.
 * @param {object} [options.fixtures] Shallow overrides for any fixture below,
 *   for a capture that needs richer seed data than the stills do.
 * @param {Record<string, unknown>} [options.commandResults] Extra IPC commands,
 *   or replacements for built-in ones, as plain serializable values. Consulted
 *   before the built-in handlers, so a caller can answer a command this mock
 *   does not know about without forking the whole invoke chain.
 */
export async function installTauriDocsMock(page, options = {}) {
  const fixtures = {
    agents, agentClasses, telemetry, telemetryFleet, telemetryMatrix,
    terminalOutput: options.terminalOutput ?? terminalOutput,
    libraryTree, libraryIndex, automations, queueItems, repoRoot,
    directoryTree, gitStatus, gitHistory, dismissedOnboardingHintIds,
    workbenchDocument, providerModelCatalog, generatedAgentNames,
    providerReadiness,
    ...options.fixtures,
  };
  const commandResults = options.commandResults ?? {};
  await page.addInitScript(({ fixtures, commandResults }) => {
    const { agents, agentClasses, telemetry, telemetryFleet, telemetryMatrix, terminalOutput, libraryTree, libraryIndex, automations, queueItems, repoRoot, directoryTree, gitStatus, gitHistory, dismissedOnboardingHintIds, workbenchDocument, providerModelCatalog, generatedAgentNames, providerReadiness } = fixtures;
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
    const pendingPty = {};
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

    /**
     * Append terminal output for one agent and tell the app to drain it.
     *
     * Video capture needs terminals that move. The stills only ever needed a
     * single paste, so `read_agent_pty` answered once and then went quiet.
     */
    tauriWindow.__WARDIAN_DOCS_PUSH_PTY = (sessionId, chunk) => {
      pendingPty[sessionId] = (pendingPty[sessionId] || "") + chunk;
      tauriWindow.__WARDIAN_DOCS_EMIT("agent-pty-output-ready", { session_id: sessionId });
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
        // Caller-supplied answers win, so a capture can seed a surface this
        // mock has no built-in handler for without forking the chain below.
        if (Object.prototype.hasOwnProperty.call(commandResults, command)) {
          return structuredClone(commandResults[command]);
        }
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
        // The spawn panel is visible in most captures. Without these four the
        // panel renders an amber "Provider returned an invalid model
        // catalogue." and a permanent "Loading generated name...", both of
        // which end up on camera.
        if (command === "list_provider_model_catalog") {
          return {
            provider: args.provider ?? "claude",
            version: null,
            source: "provider_aliases",
            models: (providerModelCatalog[args.provider] ?? providerModelCatalog.claude).map(
              (model) => ({ ...model, effort_options: [...model.effort_options] }),
            ),
            refresh_error: null,
          };
        }
        if (command === "get_generated_agent_name") {
          return generatedAgentNames[args.agentClass] ?? "Docs-Agent";
        }
        if (command === "list_provider_readiness") return providerReadiness;
        if (command === "validate_directory_path") return true;
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
        // `DirectoryTreeResult`, not a bare array. The command was paginated and
        // this mock kept returning the old shape, so `result.nodes` was
        // undefined and the Explorer crashed into the error boundary the moment
        // a tree rendered.
        if (command === "get_directory_tree") {
          return {
            nodes: directoryTree[args.path] || [],
            truncated: false,
            next_offset: null,
          };
        }
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
        // `RunSummaryListResult` and `BlueprintListResult`, not bare arrays.
        // Both commands were paginated and this mock kept returning the old
        // shape, so the automation surface read `undefined` off the result and
        // crashed into the error boundary before the canvas ever rendered.
        if (command === "automation_list_runs") {
          return { runs: [], truncated: false, next_offset: null };
        }
        if (command === "schedule_list") return [];
        if (command === "automation_list_blueprints") {
          return {
            blueprints: automations.map((automation) => ({
              id: automation.id,
              name: automation.name,
              path: `${repoRoot}/library/automations/${automation.id}.md`,
            })),
            truncated: false,
            next_offset: null,
          };
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
          if (!sessionId) return null;
          // Anything pushed since the last drain goes first, so a capture can
          // stream terminal output over time instead of pasting it all at once.
          if (pendingPty[sessionId]) {
            const pending = pendingPty[sessionId];
            pendingPty[sessionId] = "";
            return pending;
          }
          if (terminalReads[sessionId]) return null;
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
  }, { fixtures, commandResults });
}

