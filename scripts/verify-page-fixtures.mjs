#!/usr/bin/env node
/**
 * Fails when a test fixture mocks a bounded list command with a bare array.
 *
 * `get_directory_tree`, `workflow_list_blueprints`, `workflow_list_runs`,
 * `list_inbox_notifications` and `get_pair_activity` each return one struct
 * carrying its collection plus `truncated` and `next_offset`.
 *
 * The frontend used to branch on `Array.isArray` at every call site to tolerate
 * both shapes. Nothing produced the array shape except fixtures, so the branch
 * existed to keep stale mocks working — production code shaped by test data.
 * Removing the branch made those fixtures fail, and this keeps them from
 * drifting back.
 *
 * Use the helpers in `src/test/pageFixtures.ts` rather than hand-writing the
 * wrapper.
 */
import { readFileSync, readdirSync } from "node:fs";
import path from "node:path";
import process from "node:process";

/** Command name to the key its page carries. */
const PAGED_COMMANDS = Object.freeze({
  get_directory_tree: "nodes",
  workflow_list_blueprints: "blueprints",
  workflow_list_runs: "runs",
  list_inbox_notifications: "notifications",
  get_pair_activity: "pairs",
});

const violations = [];

function walk(dir, predicate, found = []) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      if (entry.name === "node_modules") continue;
      walk(full, predicate, found);
    } else if (predicate(full)) {
      found.push(full);
    }
  }
  return found;
}

// Fixtures and helpers count, not just spec files. The first version of this
// check scanned only `*.spec.ts` and missed `e2e/fixtures/workbenchIpcMock.ts`,
// whose bare-array defaults broke five workbench navigation tests.
const testFiles = [
  ...walk("src", (f) => /\.test\.tsx?$/.test(f) || f.includes(`${path.sep}test${path.sep}`)),
  ...walk("e2e", (f) => /\.(ts|tsx|mjs)$/.test(f)),
  ...walk("e2e-native", (f) => /\.mjs$/.test(f)),
];

for (const file of testFiles) {
  const lines = readFileSync(file, "utf8").split(/\r?\n/);
  lines.forEach((line, index) => {
    for (const [command, key] of Object.entries(PAGED_COMMANDS)) {
      if (!line.includes(command)) continue;

      // A response map keyed by command: `workflow_list_runs: [],`. This is the
      // form `e2e/fixtures/workbenchIpcMock.ts` uses, and the one the first
      // version of this check missed.
      const asMapEntry = new RegExp(String.raw`\b${command}\s*:\s*\[`).test(line);
      // A guard and its return on one line: `if (c === "x") return [...]`.
      const inline = new RegExp(`${command}[^\\n]*?\\breturn\\s*(\\[|\\w+\\s*;)`).exec(line);
      // A guard whose return is the next non-blank line.
      const next = lines.slice(index + 1, index + 3).find((candidate) => candidate.trim() !== "") ?? "";
      const followsWithArray = /^\s*return\s*\[/.test(next) && line.includes(command);

      const suspect = asMapEntry || (inline && inline[1].startsWith("[")) || followsWithArray;
      if (!suspect) continue;
      // Already page-shaped or routed through a helper.
      if (new RegExp(`\\b${key}\\s*:`).test(line) || /Page\s*\(/.test(line) || /Page\s*\(/.test(next)) continue;

      violations.push({
        file: file.split(path.sep).join("/"),
        line: index + 1,
        message:
          `mocks \`${command}\` with a bare array. It returns { ${key}, truncated, next_offset }; `
          + `use the helper from src/test/pageFixtures.ts.`,
      });
    }
  });
}

if (violations.length === 0) {
  console.log(
    `Page fixtures: every mock of the ${Object.keys(PAGED_COMMANDS).length} bounded list commands is page-shaped.`,
  );
  process.exit(0);
}

console.error(`Page fixtures: ${violations.length} bare-array mock(s).\n`);
for (const { file, line, message } of violations) {
  console.error(`  ${file}:${line}\n    ${message}`);
}
console.error("\nA fixture must not describe a response the backend cannot produce.");
process.exit(1);
