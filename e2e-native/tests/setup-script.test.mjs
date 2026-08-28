// @tier nightly — Runs on the nightly schedule; too slow or too broad for every pull request.
import test from "node:test";
import assert from "node:assert/strict";

import {
  edgeDriverDownloadUrl,
  msEdgeDriverToolInstallArgs,
  nativeDriverCandidates,
  nativeDriverGuidance,
  parseArgs,
  resolveCommand,
  webview2RuntimeRoots,
} from "../../scripts/setup-native-e2e.mjs";

test("native setup parses skip aliases", () => {
  assert.deepEqual(parseArgs(["--skip-tauri-driver", "--skip-edge-driver"]), {
    skipTauriDriver: true,
    skipNativeDriver: true,
    help: false,
  });
  assert.equal(parseArgs(["--skip-webdriver"]).skipNativeDriver, true);
});

test("native setup rejects unknown options", () => {
  assert.throws(() => parseArgs(["--unknown"]), /Unknown option/);
});

test("native setup resolves commands from PATH entries", () => {
  const delimiter = process.platform === "win32" ? ";" : ":";
  const env = { PATH: [process.cwd(), "C:\\missing"].join(delimiter) };
  assert.equal(resolveCommand("definitely-missing-command", env), null);
});

test("native setup provides driver candidates and guidance for supported platforms", () => {
  assert.ok(nativeDriverCandidates("win32").some((candidate) => candidate.includes("msedgedriver")));
  assert.ok(nativeDriverCandidates("linux").includes("chromedriver"));
  assert.match(nativeDriverGuidance("darwin"), /chromedriver|geckodriver/);
});

test("native setup pins git-sourced msedgedriver helper", () => {
  const args = msEdgeDriverToolInstallArgs();

  assert.deepEqual(args.slice(0, 3), [
    "install",
    "--git",
    "https://github.com/chippers/msedgedriver-tool",
  ]);
  assert.ok(args.includes("--rev"));
  assert.match(args[args.indexOf("--rev") + 1], /^[0-9a-f]{40}$/);
  assert.ok(args.includes("--locked"));
});

test("native setup derives the Edge WebDriver URL from the runtime version", () => {
  assert.equal(
    edgeDriverDownloadUrl("150.0.4078.105", "x64"),
    "https://msedgedriver.microsoft.com/150.0.4078.105/edgedriver_win64.zip",
  );
  assert.equal(
    edgeDriverDownloadUrl("150.0.4078.105", "arm64"),
    "https://msedgedriver.microsoft.com/150.0.4078.105/edgedriver_arm64.zip",
  );
});

test("native setup searches the installed WebView2 roots", () => {
  assert.deepEqual(webview2RuntimeRoots({
    ProgramFiles: "C:\\Program Files",
    "ProgramFiles(x86)": "C:\\Program Files (x86)",
    LOCALAPPDATA: "C:\\Users\\runneradmin\\AppData\\Local",
  }), [
    "C:\\Program Files\\Microsoft\\EdgeWebView\\Application",
    "C:\\Program Files (x86)\\Microsoft\\EdgeWebView\\Application",
    "C:\\Users\\runneradmin\\AppData\\Local\\Microsoft\\EdgeWebView\\Application",
  ]);
});
