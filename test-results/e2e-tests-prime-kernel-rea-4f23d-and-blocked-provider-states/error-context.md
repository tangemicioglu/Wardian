# Instructions

- Following Playwright test failed.
- Explain why, be concise, respect Playwright best practices.
- Provide a snippet of code with the fix, if possible.

# Test info

- Name: e2e\tests\prime-kernel-readiness-screenshot.spec.ts >> Prime Agent kernel readiness evidence >> captures the provisioning and blocked provider states
- Location: e2e\tests\prime-kernel-readiness-screenshot.spec.ts:106:3

# Error details

```
Error: page.goto: Protocol error (Page.navigate): Cannot navigate to invalid URL
Call log:
  - navigating to "/", waiting until "domcontentloaded"

```

# Test source

```ts
  1   | /**
  2   |  * PR evidence for the Prime Agent kernel readiness states.
  3   |  *
  4   |  * Captures the two labels a user can now see for an installed provider whose
  5   |  * runtime dependency is missing. Before this change both rendered as
  6   |  * "Prime Agent - not installed", which told the user to reinstall software they
  7   |  * already had.
  8   |  */
  9   | 
  10  | import { test, type Page } from "@playwright/test";
  11  | import * as path from "path";
  12  | 
  13  | type ReadinessFixture = {
  14  |   provider: string;
  15  |   display_name: string;
  16  |   available: boolean;
  17  |   executable: string | null;
  18  |   reason: string | null;
  19  | };
  20  | 
  21  | const SETTING_UP: ReadinessFixture = {
  22  |   provider: "prime",
  23  |   display_name: "Prime Agent",
  24  |   available: false,
  25  |   executable: "C:/Users/dev/AppData/Roaming/npm/prime-agent",
  26  |   reason:
  27  |     "Wardian is setting up Prime Agent's Python kernel. This runs once and takes a minute; Prime Agent becomes available when it finishes.",
  28  | };
  29  | 
  30  | const NEEDS_KERNEL: ReadinessFixture = {
  31  |   provider: "prime",
  32  |   display_name: "Prime Agent",
  33  |   available: false,
  34  |   executable: "C:/Users/dev/AppData/Roaming/npm/prime-agent",
  35  |   reason:
  36  |     "Wardian could not set up Prime Agent's Python kernel because `uv` is not installed. Install uv (https://docs.astral.sh/uv/) and restart Wardian, or set PRIME_AGENT_KERNEL_PYTHON to an interpreter that already has ipykernel and prime-agent-runtime.",
  37  | };
  38  | 
  39  | async function installReadinessMock(page: Page, prime: ReadinessFixture) {
  40  |   await page.addInitScript((primeReadiness) => {
  41  |     const tauriWindow = window as unknown as Record<string, unknown>;
  42  |     const callbacks = new Map<number, unknown>();
  43  |     let callbackId = 1;
  44  | 
  45  |     tauriWindow.__TAURI_INTERNALS__ = {
  46  |       metadata: {
  47  |         currentWindow: { label: "main" },
  48  |         currentWebview: { label: "main" },
  49  |       },
  50  |       transformCallback: (callback: unknown) => {
  51  |         const id = callbackId++;
  52  |         callbacks.set(id, callback);
  53  |         return id;
  54  |       },
  55  |       unregisterCallback: (id: number) => {
  56  |         callbacks.delete(id);
  57  |       },
  58  |       convertFileSrc: (filePath: string) => filePath,
  59  |       invoke: async (command: string) => {
  60  |         if (command === "list_provider_readiness") {
  61  |           return [
  62  |             {
  63  |               provider: "claude",
  64  |               display_name: "Claude",
  65  |               available: true,
  66  |               executable: "claude",
  67  |               reason: null,
  68  |             },
  69  |             {
  70  |               provider: "codex",
  71  |               display_name: "Codex",
  72  |               available: false,
  73  |               executable: null,
  74  |               reason: "Codex is not available because the codex command was not found.",
  75  |             },
  76  |             primeReadiness,
  77  |           ];
  78  |         }
  79  |         if (command === "list_agents") return [];
  80  |         if (command === "list_agent_classes") {
  81  |           return [{ name: "Architect", description: "Designs systems", is_default: false }];
  82  |         }
  83  |         if (command === "load_onboarding_hints") {
  84  |           return { dismissed_hint_ids: ["spawn-agent-first-run:v1"] };
  85  |         }
  86  |         if (command === "load_watchlists") return [];
  87  |         if (command === "load_queue_items") return [];
  88  |         if (command === "list_workflows") return [];
  89  |         return null;
  90  |       },
  91  |     };
  92  |   }, prime);
  93  | }
  94  | 
  95  | async function captureProviderList(page: Page, name: string, outputDir: string) {
> 96  |   await page.goto("/", { waitUntil: "domcontentloaded" });
      |              ^ Error: page.goto: Protocol error (Page.navigate): Cannot navigate to invalid URL
  97  |   await page.locator('[data-testid="app-shell"]').waitFor({ timeout: 15_000 });
  98  |   await page.locator('[data-testid="sidebar-tab-agent-config"]').click();
  99  |   const select = page.locator('[data-testid="spawn-provider"]');
  100 |   await select.waitFor({ timeout: 15_000 });
  101 | 
  102 |   await page.screenshot({ path: path.join(outputDir, `${name}.png`) });
  103 | }
  104 | 
  105 | test.describe("Prime Agent kernel readiness evidence", () => {
  106 |   test("captures the provisioning and blocked provider states", async ({ page }, testInfo) => {
  107 |     const stamp = new Date().toISOString().replace(/[:.]/g, "-");
  108 |     const outputDir = path.join(
  109 |       testInfo.project.testDir,
  110 |       "..",
  111 |       "screenshots",
  112 |       "prime-kernel-readiness",
  113 |       stamp,
  114 |     );
  115 | 
  116 |     await installReadinessMock(page, SETTING_UP);
  117 |     await captureProviderList(page, "prime-kernel-provisioning", outputDir);
  118 | 
  119 |     await page.context().clearCookies();
  120 |     await installReadinessMock(page, NEEDS_KERNEL);
  121 |     await captureProviderList(page, "prime-kernel-needs-setup", outputDir);
  122 | 
  123 |     testInfo.annotations.push({ type: "screenshots", description: outputDir });
  124 |     console.log(`screenshots: ${outputDir}`);
  125 |   });
  126 | });
  127 | 
```