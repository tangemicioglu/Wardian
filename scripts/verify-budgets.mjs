#!/usr/bin/env node
/**
 * Debt budgets: numbers that may fall but never rise.
 *
 * Some of what the merged-PR audit found cannot be fixed in one change. A
 * 10,000-line command module will not be split in a single pull request, and
 * should not be. What can be done immediately is stop it growing.
 *
 * Each metric below is frozen at its measured value. CI fails when a number
 * exceeds its budget. A change that improves one lowers it in the same commit,
 * which is the only way a budget ever moves. That makes the ratchet the one
 * mechanism here that gets stricter on its own.
 *
 * Usage:
 *   node scripts/verify-budgets.mjs           check against budgets.json
 *   node scripts/verify-budgets.mjs --write   re-freeze at current values
 */
import { execFileSync } from "node:child_process";
import { readFileSync, readdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import process from "node:process";

const BUDGETS_PATH = "budgets.json";
const WRITE = process.argv.includes("--write");

function walk(dir, predicate, found = []) {
  let entries;
  try {
    entries = readdirSync(dir, { withFileTypes: true });
  } catch {
    return found;
  }
  for (const entry of entries) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      if (["node_modules", "target", "dist", ".git"].includes(entry.name)) continue;
      walk(full, predicate, found);
    } else if (predicate(full)) {
      found.push(full);
    }
  }
  return found;
}

const posix = (p) => p.split(path.sep).join("/");

/** Rust sources, excluding generated and vendored trees. */
function rustFiles() {
  return [...walk("src-tauri/src", (f) => f.endsWith(".rs")), ...walk("crates", (f) => f.endsWith(".rs"))];
}

function countMatches(files, pattern) {
  let total = 0;
  for (const file of files) {
    const matches = readFileSync(file, "utf8").match(pattern);
    total += matches ? matches.length : 0;
  }
  return total;
}

// ---- Metrics -------------------------------------------------------------

/**
 * Per-file line counts for the modules that only ever grew.
 *
 * Per-file rather than a single total: one file shrinking must not buy room
 * for another to grow.
 */
function fileLines(tracked) {
  const measured = {};
  for (const file of Object.keys(tracked)) {
    try {
      measured[file] = readFileSync(file, "utf8").split("\n").length;
    } catch {
      // A tracked file that no longer exists is an improvement, not a failure.
      measured[file] = 0;
    }
  }
  return measured;
}

function measure(budgets) {
  const rust = rustFiles();
  const eslint = JSON.parse(
    // Run ESLint's own entry point rather than `npx`, which is a shell shim on
    // Windows and cannot be spawned without `shell: true`.
    execFileSync(
      process.execPath,
      [path.join("node_modules", "eslint", "bin", "eslint.js"), ".", "-f", "json"],
      { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 },
    ),
  );
  const warnings = eslint.reduce((sum, file) => sum + file.warningCount, 0);

  return {
    file_lines: fileLines(budgets.file_lines ?? {}),
    // Suppressing a lint is sometimes right and always worth counting.
    clippy_allow_too_many_arguments: countMatches(rust, /#\[allow\(clippy::too_many_arguments\)\]/g),
    clippy_allow_await_holding_lock: countMatches(rust, /#\[allow\(clippy::await_holding_lock\)\]/g),
    // A `#[cfg(test)]` free function is usually a test seam, occasionally a
    // fork of shipping logic. The count stops new ones arriving unexamined.
    cfg_test_functions: countMatches(rust, /#\[cfg\(test\)\]\s*\n\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s/g),
    // Rust tests that only run behind `--ignored`.
    ignored_rust_tests: countMatches(rust, /#\[ignore\b/g),
    // Browser E2E cases disabled outright.
    skipped_e2e_tests: countMatches(
      walk("e2e", (f) => f.endsWith(".spec.ts")),
      /\btest\.skip\s*\(\s*(?:true\b|["'`])/g,
    ),
    eslint_warnings: warnings,
  };
}

// ---- Compare -------------------------------------------------------------

const budgets = JSON.parse(readFileSync(BUDGETS_PATH, "utf8"));
const measured = measure(budgets);

if (WRITE) {
  writeFileSync(BUDGETS_PATH, `${JSON.stringify({ ...budgets, ...measured }, null, 2)}\n`);
  console.log(`Re-froze ${BUDGETS_PATH} at current values.`);
  process.exit(0);
}

const over = [];
const under = [];

for (const [key, value] of Object.entries(measured)) {
  if (key === "file_lines") {
    for (const [file, lines] of Object.entries(value)) {
      const budget = budgets.file_lines[posix(file)] ?? budgets.file_lines[file];
      if (budget === undefined) continue;
      if (lines > budget) over.push([`${posix(file)} lines`, lines, budget]);
      else if (lines < budget) under.push([`${posix(file)} lines`, lines, budget]);
    }
    continue;
  }
  const budget = budgets[key];
  if (budget === undefined) continue;
  if (value > budget) over.push([key, value, budget]);
  else if (value < budget) under.push([key, value, budget]);
}

if (over.length > 0) {
  console.error(`Debt budgets: ${over.length} metric(s) above budget.\n`);
  for (const [name, value, budget] of over) {
    console.error(`  ${name}: ${value} (budget ${budget}, +${value - budget})`);
  }
  console.error(
    "\nThese numbers may fall but never rise. Either bring the change under "
      + "budget, or make the case for the increase and raise it deliberately.",
  );
  process.exit(1);
}

console.log(`Debt budgets: all ${Object.keys(measured).length} metric group(s) within budget.`);
if (under.length > 0) {
  console.log(`\n${under.length} metric(s) improved. Re-freeze with: npm run check:budgets -- --write\n`);
  for (const [name, value, budget] of under) {
    console.log(`  ${name}: ${value} (budget ${budget}, −${budget - value})`);
  }
}
