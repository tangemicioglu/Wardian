import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";

/**
 * Tiers a native E2E test can declare.
 *
 * `ci` runs on every pull request, `nightly` on the schedule, and `manual`
 * needs a real provider or a logged-in CLI. CI selects a tier rather than
 * naming files, so adding a test forces the question of when it runs instead
 * of leaving it to run nowhere.
 */
export const NATIVE_E2E_TIERS = Object.freeze(["ci", "nightly", "manual"]);

const TIER_PATTERN = /^\s*\/\/\s*@tier\s+([a-z]+)/m;

/** Reads the `// @tier <tier>` declaration from a native test file. */
export function nativeE2eTier(file) {
  const match = TIER_PATTERN.exec(readFileSync(file, "utf8"));
  if (!match) return null;
  return NATIVE_E2E_TIERS.includes(match[1]) ? match[1] : match[1];
}

/**
 * Native E2E test files, optionally filtered to one tier.
 *
 * Passing no tier returns every test, which is what the local full run wants.
 */
export function nativeE2eTestTargets({ tier } = {}) {
  const testDir = path.join("e2e-native", "tests");
  return readdirSync(testDir)
    .filter((entry) => entry.endsWith(".test.mjs"))
    .sort()
    .map((entry) => path.join(testDir, entry))
    .filter((file) => (tier ? nativeE2eTier(file) === tier : true));
}
