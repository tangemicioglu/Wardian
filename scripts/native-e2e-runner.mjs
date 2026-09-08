import { spawn, spawnSync } from "node:child_process";

import {
  NATIVE_E2E_HOME_ENV,
  NATIVE_E2E_RUN_ID_ENV,
  acquireHomeLock,
  releaseHomeLock,
  resolveRunNativeHome,
} from "../e2e-native/lib/sessionHome.mjs";

const NODE_TEST_ARGS = ["--test", "--test-concurrency=1"];

export { resolveRunNativeHome };

export function createNativeE2eRunPlans({ requestedTargets, defaultTargets }) {
  const targets = requestedTargets.length > 0 ? requestedTargets : defaultTargets;
  return targets.map((target) => ({
    command: process.execPath,
    args: [...NODE_TEST_ARGS, target],
  }));
}

function runChild(plan, env) {
  return new Promise((resolve) => {
    const child = spawn(plan.command, plan.args, {
      stdio: "inherit",
      env,
      // A process group makes the whole tree addressable on POSIX, so cleanup
      // can end exactly what this runner started and nothing else.
      detached: process.platform !== "win32",
    });
    const pid = child.pid;

    child.on("exit", (code, signal) => {
      resolve({ result: { code: code ?? 1, signal }, pid });
    });
  });
}

/**
 * Terminate one process tree this runner started.
 *
 * This replaces a sweep that enumerated every process whose command line
 * contained the home path and force-stopped it. That matched by coincidence,
 * not ownership: an unrelated process that merely mentions the path was killed,
 * and two runs sharing an explicit home killed each other. Only the tree rooted
 * at a pid this runner spawned is ours to end.
 */
export function createOwnedTreeTerminationPlan(pid, platform = process.platform) {
  if (!Number.isInteger(pid) || pid <= 0) {
    return null;
  }
  if (platform === "win32") {
    return { command: "taskkill.exe", args: ["/PID", String(pid), "/T", "/F"] };
  }
  return { command: "kill", args: ["-TERM", `-${pid}`] };
}

function terminateOwnedTree(pid, platform = process.platform) {
  const plan = createOwnedTreeTerminationPlan(pid, platform);
  if (!plan) {
    return;
  }
  try {
    spawnSync(plan.command, plan.args, { stdio: "ignore" });
  } catch {
    // The tree is already gone; nothing else is ours to end.
  }
}

export async function runNativeE2eTargets({
  requestedTargets,
  defaultTargets,
  env = process.env,
}) {
  const plans = createNativeE2eRunPlans({ requestedTargets, defaultTargets });
  // Pin the home once, before any target runs, and hand the same value to every
  // child so the runner and the harness never disagree about which home is in
  // play. Isolation therefore applies to the ordinary npm path with no manual
  // port or home selection.
  const { home, generated, runId } = resolveRunNativeHome(env);

  // Claim the home BEFORE anything destructive happens. Cleanup used to run
  // ahead of the test child, so a refusal raised later inside the harness
  // arrived after a second run had already terminated the first run's
  // processes. Refusing here is what makes the guarantee real.
  const { staleHolder } = acquireHomeLock({ home, runId });
  if (staleHolder) {
    console.warn(
      `[native-e2e] Reusing ${home}; it was left locked by run ${staleHolder.runId ?? "unknown"} ` +
        `(pid ${staleHolder.pid}), which is no longer running. Processes orphaned by that run are not ` +
        `terminated automatically, because they cannot be told apart from unrelated processes.`,
    );
  }

  // The run id always reaches the child, including for an explicit home. The
  // child has to recognise the runner's claim as its own, and identity is the
  // run id rather than a pid that differs between runner and child.
  const runEnv = { ...env, [NATIVE_E2E_HOME_ENV]: home, [NATIVE_E2E_RUN_ID_ENV]: runId };

  try {
    for (const plan of plans) {
      const { result, pid } = await runChild(plan, runEnv);
      // End only the tree we started, and only after it has exited, so a child
      // that leaked the app or driver cannot outlive the run.
      terminateOwnedTree(pid);
      if (result.signal) {
        process.kill(process.pid, result.signal);
        return 1;
      }
      if (result.code !== 0) {
        return result.code;
      }
    }
  } finally {
    releaseHomeLock({ home, runId });
  }

  return 0;
}
