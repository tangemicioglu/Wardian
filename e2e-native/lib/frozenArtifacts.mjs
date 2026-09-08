import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";

/** Directory inside a run's home holding that run's private binaries. */
export const FROZEN_BIN_DIR = ".frozen-bin";

function sha256(filePath) {
  return crypto.createHash("sha256").update(fs.readFileSync(filePath)).digest("hex");
}

/**
 * Binaries a Windows Tauri build needs beside the executable.
 *
 * Copying the executable alone produces something that cannot start, so the
 * adjacent link libraries travel with it.
 */
function sidecarsFor(sourcePath) {
  const dir = path.dirname(sourcePath);
  let entries = [];
  try {
    entries = fs.readdirSync(dir, { withFileTypes: true });
  } catch {
    return [];
  }
  return entries
    .filter((entry) => entry.isFile() && /\.(dll|so|dylib)$/i.test(entry.name))
    .map((entry) => path.join(dir, entry.name));
}

/**
 * Copy one binary and its sidecars into a run-private directory.
 *
 * Recording a hash of the original only attributes what a run started with; it
 * does not stop another worktree rebuilding that path midway and changing the
 * bytes underneath a live session. The normal build target is shared, so a run
 * takes its own copy and executes that instead. The recorded identity ties the
 * copy back to the source it came from.
 */
export function freezeArtifact(sourcePath, destDir) {
  if (!sourcePath || !fs.existsSync(sourcePath)) {
    return null;
  }
  fs.mkdirSync(destDir, { recursive: true });

  for (const sidecar of sidecarsFor(sourcePath)) {
    const target = path.join(destDir, path.basename(sidecar));
    if (!fs.existsSync(target)) {
      fs.copyFileSync(sidecar, target);
    }
  }

  const frozenPath = path.join(destDir, path.basename(sourcePath));
  fs.copyFileSync(sourcePath, frozenPath);
  const stats = fs.statSync(frozenPath);
  return {
    path: frozenPath,
    source: sourcePath,
    sha256: sha256(frozenPath),
    bytes: stats.size,
    frozenAt: new Date().toISOString(),
  };
}

/**
 * Freeze every binary a run executes, into that run's own home.
 *
 * The home is per-run and is removed with the run, so the copies are cleaned up
 * without any extra bookkeeping.
 */
export function freezeRunArtifacts({ home, appPath, cliPath }) {
  const destDir = path.join(home, FROZEN_BIN_DIR);
  const app = freezeArtifact(appPath, destDir);
  const cli = cliPath ? freezeArtifact(cliPath, destDir) : null;
  return { dir: destDir, app, cli };
}
