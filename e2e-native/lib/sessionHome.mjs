import fs from "node:fs";
import os from "node:os";
import path from "node:path";

export const NATIVE_E2E_HOME_ENV = "WARDIAN_E2E_NATIVE_HOME";
export const NATIVE_E2E_RUN_ID_ENV = "WARDIAN_E2E_RUN_ID";
export const NATIVE_E2E_HOME_PREFIX = "wardian-e2e-native-";
export const HOME_LOCK_FILE = ".native-e2e-lock.json";

/** Identifies one run so its home, ports and children are attributable. */
export function nativeRunId() {
  return `${process.pid.toString(36)}${Math.random().toString(36).slice(2, 8)}`;
}

export function defaultNativeE2EHome(runId) {
  return path.join(os.tmpdir(), `${NATIVE_E2E_HOME_PREFIX}${runId}`);
}

/**
 * The home this run owns.
 *
 * An explicit home is honored so a caller can inspect state afterwards.
 * Otherwise each run gets its own, which is what lets two runs coexist.
 */
export function resolveRunNativeHome(env = process.env) {
  // A run always has an id, including when the home was supplied explicitly.
  // The id, not the pid, is what identifies a run: the runner claims the home
  // and then spawns a child that must recognise the claim as its own.
  const runId = env[NATIVE_E2E_RUN_ID_ENV] || nativeRunId();
  if (env[NATIVE_E2E_HOME_ENV]) {
    return { home: env[NATIVE_E2E_HOME_ENV], generated: false, runId };
  }
  return { home: defaultNativeE2EHome(runId), generated: true, runId };
}

/**
 * A lock holder is only real while its process is still alive.
 *
 * This deliberately does not exempt the calling process. Whether a lock belongs
 * to the caller is decided by run id, not pid, because one run spans the runner
 * and the child it spawns. Exempting our own pid here would hide a live holder
 * from a genuinely different run in the same process.
 */
export function lockHolderAlive(pid) {
  if (!Number.isInteger(pid) || pid <= 0) {
    return false;
  }
  try {
    process.kill(pid, 0);
    return true;
  } catch (error) {
    // EPERM means the process exists but is owned by someone else.
    return Boolean(error) && error.code === "EPERM";
  }
}

export function readHomeLock(homePath) {
  try {
    return JSON.parse(fs.readFileSync(path.join(homePath, HOME_LOCK_FILE), "utf8"));
  } catch {
    return null;
  }
}

/**
 * Claim a home before anything destructive touches it.
 *
 * Ownership has to be established here, in the runner, rather than later when
 * the harness resets the directory. Cleanup runs before the test child starts,
 * so a refusal that only happens inside the harness arrives after the damage:
 * a second run pointed at the same explicit home would already have terminated
 * the first run's processes.
 *
 * Returns the lock this run now holds. Throws if a live foreign holder exists.
 */
export function acquireHomeLock({ home, runId, pid = process.pid }) {
  const existing = readHomeLock(home);
  // A run spans more than one process: the runner claims the home, then spawns
  // the test child that opens the session. Identity is therefore the run id.
  // Comparing pids instead made a run refuse its own home, because the child
  // saw its live parent as a foreign holder.
  const heldByThisRun = Boolean(existing) && existing.runId != null && existing.runId === runId;
  if (existing && !heldByThisRun && lockHolderAlive(existing.pid)) {
    throw new Error(
      `Refusing to use native E2E home ${home}: run ${existing.runId ?? "unknown"} (pid ${existing.pid}, ` +
        `started ${existing.startedAt ?? "unknown"}) is still using it. Give each concurrent run its own ` +
        `${NATIVE_E2E_HOME_ENV}, or leave it unset to get an isolated home automatically.`,
    );
  }

  const lock = {
    runId: runId ?? null,
    pid,
    // Keep the first claim's timestamp so the record shows when the run began.
    startedAt: heldByThisRun && existing.startedAt ? existing.startedAt : new Date().toISOString(),
  };
  fs.mkdirSync(home, { recursive: true });
  fs.writeFileSync(path.join(home, HOME_LOCK_FILE), `${JSON.stringify(lock)}\n`);
  return {
    lock,
    reclaimed: heldByThisRun,
    staleHolder: existing && !heldByThisRun && !lockHolderAlive(existing.pid) ? existing : null,
  };
}

/** Release this run's claim; never remove a lock another run owns. */
export function releaseHomeLock({ home, runId, pid = process.pid }) {
  const lock = readHomeLock(home);
  const ours = lock && (runId != null ? lock.runId === runId : lock.pid === pid);
  if (ours) {
    try {
      fs.rmSync(path.join(home, HOME_LOCK_FILE), { force: true });
    } catch {
      // A home already removed by teardown needs no release.
    }
  }
}
