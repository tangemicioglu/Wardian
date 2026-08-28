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
/**
 * The first `return` inside the block a command guard opens.
 *
 * Returns null when the line is not a guard, or when the block has no return.
 * Brace counting is enough here: these are hand-written mock bodies, not
 * arbitrary source.
 */
function returnInGuardBlock(lines, index, command) {
  const start = lines[index];
  if (!new RegExp(String.raw`${command}["'\s)]*\)?\s*\{\s*$`).test(start)) return null;
  let depth = 1;
  for (let i = index + 1; i < lines.length && depth > 0; i += 1) {
    const current = lines[i];
    const match = /^\s*return\s/.exec(current);
    if (match && depth === 1) return current;
    depth += (current.match(/\{/g) ?? []).length;
    depth -= (current.match(/\}/g) ?? []).length;
  }
  return null;
}

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

      // A guard whose return is further down its block. Checking only the next
      // line or two is not enough: `workbenchIpcMock.ts` builds the listing over
      // twenty lines before returning it, and that gap is what let the Explorer
      // tree break with this check passing.
      const blockReturn = returnInGuardBlock(lines, index, command);

      // `return activityFixture;` — a variable, so there is no literal to
      // match. The Graph inspector broke this way: the helper took
      // `PairActivity[]` and handed it straight back. A page-shaped value is an
      // object literal or a `*Page(...)` call, so a bare identifier is wrong
      // here — except for the absent-value returns a mock legitimately makes.
      const ABSENT = /^(null|undefined)$/;
      const identifierReturn = (candidate) =>
        candidate !== null
        && /^\s*return\s+([A-Za-z_$][\w$]*)\s*;\s*$/.test(candidate)
        && !ABSENT.test(/^\s*return\s+([A-Za-z_$][\w$]*)\s*;\s*$/.exec(candidate)[1]);

      const returnsIdentifier = identifierReturn(blockReturn)
        || (inline !== null && inline[1].endsWith(";") && !ABSENT.test(inline[1].replace(";", "").trim()));

      const suspect = asMapEntry
        || (inline && inline[1].startsWith("["))
        || returnsIdentifier
        || (blockReturn !== null && /^\s*return\s*\[/.test(blockReturn));
      if (!suspect) continue;
      // Already page-shaped or routed through a helper.
      const shaped = new RegExp(`\\b${key}\\s*:`);
      if (shaped.test(line) || /Page\s*\(/.test(line)) continue;
      if (blockReturn !== null && (shaped.test(blockReturn) || /Page\s*\(/.test(blockReturn))) continue;

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
