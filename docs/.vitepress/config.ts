import { defineConfig } from "vitepress";

const base = process.env.DOCS_BASE ?? "/";

const guideSidebar = [
  {
    text: "Start and Orient",
    items: [
      { text: "First-Time Install and First Run", link: "/guide/getting-started" },
      { text: "Key Concepts", link: "/guide/key-concepts" },
      { text: "Provider Readiness", link: "/guide/provider-readiness" },
      { text: "First-Run Troubleshooting", link: "/guide/first-run-troubleshooting" },
      { text: "UI Overview", link: "/guide/ui-overview" },
      { text: "Workbench", link: "/guide/workbench" },
    ],
  },
  {
    text: "Run and Monitor Agents",
    items: [
      { text: "Agents Overview", link: "/guide/agents-overview" },
      { text: "Grid Compatibility", link: "/guide/grid" },
      { text: "Dashboard", link: "/guide/dashboard" },
      { text: "Graph", link: "/guide/graph" },
      { text: "Garden", link: "/guide/garden" },
      { text: "Watchlists", link: "/guide/watchlists" },
      { text: "Inbox", link: "/guide/inbox" },
      { text: "Class Management", link: "/guide/class-management" },
    ],
  },
  {
    text: "Reuse and Direct Work",
    items: [
      { text: "Library", link: "/guide/library" },
      { text: "Agent Memory", link: "/guide/agent-memory" },
      { text: "Command Panel", link: "/guide/command-panel" },
      { text: "Wardian CLI", link: "/guide/cli" },
    ],
  },
  {
    text: "Inspect and Ship Changes",
    items: [
      { text: "Explorer", link: "/guide/explorer" },
      { text: "Browser", link: "/guide/browser" },
      { text: "Source Control", link: "/guide/source-control" },
    ],
  },
  {
    text: "Configure and Automate",
    items: [
      { text: "Settings", link: "/guide/settings" },
      { text: "Remote Control", link: "/guide/remote-control" },
      { text: "Automation View", link: "/guide/automations" },
      { text: "Automation Reference", link: "/automations/" },
    ],
  },
];

export default defineConfig({
  title: "Wardian",
  description:
    "Public documentation for running, monitoring, and organizing local coding agents.",
  base,
  vite: {
    build: {
      // Keep docs compatible with the patched esbuild used by the app build.
      target: "es2022",
    },
  },
  srcExclude: ["specs/**/*.md", "research/**/*.md"],
  cleanUrls: true,
  lastUpdated: true,
  ignoreDeadLinks: [
    // Specs intentionally reference future and external planning artifacts.
    /ROADMAP\.md/,
  ],
  head: [
    ["meta", { name: "theme-color", content: "#2f6f6a" }],
    ["meta", { property: "og:type", content: "website" }],
    ["meta", { property: "og:site_name", content: "Wardian Docs" }],
  ],
  themeConfig: {
    siteTitle: "Wardian Docs",
    search: {
      provider: "local",
    },
    nav: [
      { text: "Guide", link: "/guide/" },
      { text: "Automations", link: "/automations/" },
      { text: "Providers", link: "/providers" },
      { text: "Developer", link: "/developer/" },
      {
        text: "GitHub",
        link: "https://github.com/tangemicioglu/Wardian",
      },
    ],
    sidebar: {
      "/guide/": guideSidebar,
      "/automations/": [
        {
          text: "Automation Reference",
          items: [
            { text: "Overview", link: "/automations/" },
            { text: "Building Automations", link: "/automations/building-automations" },
            { text: "Agent Assignment", link: "/automations/agent-assignment" },
            { text: "Triggers", link: "/automations/triggers" },
            { text: "Scheduled Runs", link: "/automations/scheduled-runs" },
            { text: "Node Reference", link: "/automations/node-reference" },
            { text: "Troubleshooting", link: "/automations/troubleshooting" },
          ],
        },
      ],
      "/developer/": [
        {
          text: "Developer Docs",
          items: [
            { text: "Overview", link: "/developer/" },
            { text: "Architecture", link: "/developer/architecture" },
            { text: "Setup", link: "/developer/setup" },
            { text: "State Management", link: "/developer/state-management" },
            { text: "Agent Memory", link: "/developer/agent-memory" },
            { text: "IPC Events", link: "/developer/ipc-events" },
            { text: "Tauri Commands", link: "/developer/tauri-command-reference" },
            { text: "Provider Runtimes", link: "/developer/provider-runtimes" },
            { text: "PTY Lifecycle", link: "/developer/pty-lifecycle" },
            { text: "Native E2E", link: "/developer/native-e2e" },
            { text: "Release Updates", link: "/developer/release-updates" },
            { text: "Package Manager Distribution", link: "/developer/package-manager-distribution" },
            { text: "Docs Maintenance", link: "/developer/docs-maintenance" },
            { text: "Theming", link: "/developer/theming" },
            { text: "Screenshot Documentation", link: "/developer/screenshot-documentation" },
            { text: "Site Media Capture", link: "/developer/site-media" },
          ],
        },
      ],
      "/": [
        {
          text: "Start Here",
          items: [
            { text: "Documentation Index", link: "/" },
            { text: "First-Time Install and First Run", link: "/guide/getting-started" },
            { text: "Providers", link: "/providers" },
            { text: "OS Support", link: "/os-support" },
          ],
        },
        ...guideSidebar,
        {
          text: "More Documentation",
          items: [
            { text: "Key Features", link: "/features" },
            { text: "Automation Reference", link: "/automations/" },
            { text: "Developer Documentation", link: "/developer/" },
          ],
        },
      ],
    },
    socialLinks: [
      { icon: "github", link: "https://github.com/tangemicioglu/Wardian" },
    ],
    footer: {
      message: "Released under the MIT License.",
      copyright: "Copyright Tan Gemicioglu",
    },
  },
});
