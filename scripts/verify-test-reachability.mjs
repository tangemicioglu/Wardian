#!/usr/bin/env node
/**
 * Fails when a test exists that no automated run will ever execute.
 *
 * The audit of PRs #949–#1015 found two ways a test can be written, counted as
 * coverage in a PR description, and then never run: a native test CI does not
 * name, and a `test.skip` whose deferral nobody tracks.
 *
 * These rules check that a decision was recorded, not that it was the right
 * decision — a reviewer still has to read the reason. The related count-based
 * limits (ignored Rust tests, `#[cfg(test)]` seams) live in `budgets.json`,
 * because those cannot be driven to zero in one change.
 */
import { readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import process from "node:process";

import { NATIVE_E2E_TIERS, nativeE2eTier } from "./native-e2e-targets.mjs";

/**
 * Skips that predate this check, with the reason each is still here.
 *
 * This list may only shrink. Each entry needs a tracking issue and then an
 * `#issue` reference in the test itself, at which point its entry comes out.
 */
const GRANDFATHERED_SKIPS = Object.freeze([
  {
    file: "e2e/tests/agent-lifecycle.spec.ts",
    reason:
      "Nine lifecycle and status-indicator cases are empty bodies. The file's own comment "
      + "states the unlock path: add \"mock\" to SpawnAgentPanel provider options and use the "
      + "seededHome() fixture.",
  },
  {
    file: "e2e/tests/watchlist.spec.ts",
    reason:
      "Status-indicator colours need live telemetry. `search filters agents by name` is "
      + "mislabelled — it is pure UI and duplicates a passing test above it.",
  },
  {
    file: "e2e/tests/workflow.spec.ts",
    reason: "Workflow lifecycle needs a running mock agent, same blocker as agent-lifecycle.",
  },
  {
    file: "e2e/tests/graph-topology.spec.ts",
    reason: "Ghost edges need pair activity the browser mock layer does not serve.",
  },
  {
    file: "e2e/tests/library-redesign.spec.ts",
    reason: "Junction behaviour is covered by library-deployment-native.test.mjs.",
  },
]);

const ISSUE_REF = /#\d+/;
const TAG = /@native-only|@real-provider-only/;
const violations = [];
const grandfathered = new Set(GRANDFATHERED_SKIPS.map((entry) => entry.file));
const usedGrandfathers = new Set();

function report(file, line, message) {
  violations.push({ file: file.split(path.sep).join("/"), line, message });
}

function walk(dir, predicate, found = []) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      if (entry.name === "node_modules" || entry.name === "target") continue;
      walk(full, predicate, found);
    } else if (predicate(full)) {
      found.push(full);
    }
  }
  return found;
}

/** Returns the source of a `test.skip(...)` call starting at `index`. */
function callText(source, index) {
  let depth = 0;
  for (let i = index; i < source.length; i += 1) {
    const ch = source[i];
    if (ch === "(") depth += 1;
    else if (ch === ")") {
      depth -= 1;
      if (depth === 0) return source.slice(index, i + 1);
    } else if (ch === '"' || ch === "'" || ch === "`") {
      const quote = ch;
      i += 1;
      while (i < source.length && source[i] !== quote) {
        if (source[i] === "\\") i += 1;
        i += 1;
      }
    }
  }
  return source.slice(index);
}

// ---- Rule 1: every native E2E test declares a tier ------------------------
//
// CI names four of 46 native files by hand, so adding a native test adds it to
// no job. Selecting by tier only helps if every file carries one.

for (const file of readdirSync(path.join("e2e-native", "tests"))
  .filter((entry) => entry.endsWith(".test.mjs"))
  .map((entry) => path.join("e2e-native", "tests", entry))) {
  const tier = nativeE2eTier(file);
  if (tier === null) {
    report(file, 1, `no "// @tier" declaration. Use one of: ${NATIVE_E2E_TIERS.join(", ")}.`);
  } else if (!NATIVE_E2E_TIERS.includes(tier)) {
    report(file, 1, `unknown tier "${tier}". Use one of: ${NATIVE_E2E_TIERS.join(", ")}.`);
  }
}

// ---- Rule 2: every disabled browser E2E test records where it went --------
//
// A skip that names the layer now holding the coverage is a decision. A skip
// with no owner is an absence nobody measures. A `test.skip(condition, …)`
// whose condition is evaluated at runtime is a guard, not a disabled test.

for (const file of walk("e2e", (f) => f.endsWith(".spec.ts"))) {
  const source = readFileSync(file, "utf8");
  const relative = file.split(path.sep).join("/");
  const pattern = /\btest\.skip\s*\(/g;
  let match;
  while ((match = pattern.exec(source)) !== null) {
    const call = callText(source, match.index + match[0].length - 1);
    const line = source.slice(0, match.index).split("\n").length;
    const firstArg = call.slice(1).trimStart();
    const isLiteralTrue = /^true\b/.test(firstArg);
    const isRuntimeGuard = !isLiteralTrue && !/^["'`]/.test(firstArg);
    if (isRuntimeGuard) continue;

    // A suite-level tag on the enclosing `test.describe` covers every skip
    // inside it, so context reaches back to that header rather than a fixed
    // number of lines.
    const head = source.slice(0, match.index);
    const describeAt = head.lastIndexOf("test.describe(");
    const from = describeAt === -1
      ? Math.max(0, head.length - 600)
      : describeAt - 200 < 0 ? 0 : describeAt - 200;
    const context = `${source.slice(from, match.index)}\n${call}`;
    if (!TAG.test(context)) {
      report(file, line, "test.skip with no @native-only or @real-provider-only tag.");
      continue;
    }
    if (ISSUE_REF.test(context)) continue;
    if (grandfathered.has(relative)) {
      usedGrandfathers.add(relative);
      continue;
    }
    report(file, line, "test.skip is tagged but names no tracking issue (add e.g. #1234).");
  }
}

for (const { file } of GRANDFATHERED_SKIPS) {
  if (!usedGrandfathers.has(file)) {
    report(file, 1, "grandfathered skip entry is stale — no untracked skip left here. Remove it.");
  }
}

// ---- Report --------------------------------------------------------------

if (violations.length === 0) {
  const remaining = usedGrandfathers.size;
  console.log(
    `Test reachability: every native test has a tier and every skip is accounted for`
      + `${remaining ? ` (${remaining} file(s) still grandfathered).` : "."}`,
  );
  process.exit(0);
}

console.error(`Test reachability: ${violations.length} problem(s).\n`);
for (const { file, line, message } of violations) {
  console.error(`  ${file}:${line}\n    ${message}`);
}
console.error("\nA test no job runs is not coverage. Give it a tier, a tracking issue, or delete it.");
process.exit(1);
