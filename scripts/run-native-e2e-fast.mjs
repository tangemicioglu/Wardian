import { nativeE2eTestTargets } from "./native-e2e-targets.mjs";
import { runNativeE2eTargets } from "./native-e2e-runner.mjs";

const args = process.argv.slice(2);
const tierIndex = args.indexOf("--tier");
const tier = tierIndex === -1 ? undefined : args[tierIndex + 1];
const testTargets = tierIndex === -1 ? args : args.filter((_, i) => i !== tierIndex && i !== tierIndex + 1);

const defaultTargets = nativeE2eTestTargets({ tier });
if (tier && defaultTargets.length === 0) {
  console.error(`No native E2E tests declare "// @tier ${tier}".`);
  process.exit(1);
}

const exitCode = await runNativeE2eTargets({
  requestedTargets: testTargets,
  defaultTargets,
  env: {
    ...process.env,
    WARDIAN_NATIVE_SKIP_BUILD: "1",
  },
});
process.exit(exitCode);
