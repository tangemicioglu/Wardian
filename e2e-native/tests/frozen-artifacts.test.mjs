// @tier nightly — Runs on the nightly schedule; too slow or too broad for every nightly pull request.
import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";

import { FROZEN_BIN_DIR, freezeArtifact, freezeRunArtifacts } from "../lib/frozenArtifacts.mjs";

function scratch(label) {
  const dir = path.join(os.tmpdir(), `wardian-e2e-native-frozen-${label}-${process.pid}`);
  fs.rmSync(dir, { recursive: true, force: true });
  fs.mkdirSync(dir, { recursive: true });
  return dir;
}

/**
 * The point of freezing: the shared build target can be rewritten by another
 * worktree while a session is live. Recording a hash only attributes what the
 * run started with, so the run executes its own copy instead.
 */
test("a rebuild of the shared source cannot change what the run executes", () => {
  const sharedTarget = scratch("source");
  const home = scratch("home");
  try {
    const shared = path.join(sharedTarget, "Wardian.exe");
    fs.writeFileSync(shared, "original build");

    const frozen = freezeArtifact(shared, path.join(home, FROZEN_BIN_DIR));
    assert.ok(frozen, "freezing an existing binary returns its identity");
    assert.equal(frozen.source, shared);
    assert.notEqual(frozen.path, shared, "the run must execute a copy, not the shared path");

    // Another worktree rebuilds the shared target mid-run.
    fs.writeFileSync(shared, "a completely different build");

    assert.equal(fs.readFileSync(frozen.path, "utf8"), "original build");
    assert.equal(frozen.bytes, "original build".length);
  } finally {
    fs.rmSync(sharedTarget, { recursive: true, force: true });
    fs.rmSync(home, { recursive: true, force: true });
  }
});

test("sidecar libraries travel with the executable", () => {
  const sharedTarget = scratch("sidecar");
  const home = scratch("sidecar-home");
  try {
    fs.writeFileSync(path.join(sharedTarget, "Wardian.exe"), "app");
    fs.writeFileSync(path.join(sharedTarget, "wardian_app_lib.dll"), "lib");
    fs.writeFileSync(path.join(sharedTarget, "notes.txt"), "not a binary");

    const frozen = freezeArtifact(path.join(sharedTarget, "Wardian.exe"), path.join(home, FROZEN_BIN_DIR));
    const frozenDir = path.dirname(frozen.path);

    // Copying the executable alone would produce something that cannot start.
    assert.equal(fs.existsSync(path.join(frozenDir, "wardian_app_lib.dll")), true);
    assert.equal(fs.existsSync(path.join(frozenDir, "notes.txt")), false, "only link libraries travel");
  } finally {
    fs.rmSync(sharedTarget, { recursive: true, force: true });
    fs.rmSync(home, { recursive: true, force: true });
  }
});

test("each run freezes into its own home, so runs cannot share a binary", () => {
  const sharedTarget = scratch("shared");
  const homeA = scratch("run-a");
  const homeB = scratch("run-b");
  try {
    const app = path.join(sharedTarget, "Wardian.exe");
    const cli = path.join(sharedTarget, "wardian-cli.exe");
    fs.writeFileSync(app, "app build");
    fs.writeFileSync(cli, "cli build");

    const a = freezeRunArtifacts({ home: homeA, appPath: app, cliPath: cli });
    const b = freezeRunArtifacts({ home: homeB, appPath: app, cliPath: cli });

    assert.notEqual(a.app.path, b.app.path);
    assert.notEqual(a.cli.path, b.cli.path);
    assert.equal(a.app.sha256, b.app.sha256, "same source yields the same identity");
    assert.equal(path.dirname(a.app.path), path.join(homeA, FROZEN_BIN_DIR));

    // The CLI is frozen too: consumers resolve it from the same shared target.
    assert.equal(fs.readFileSync(a.cli.path, "utf8"), "cli build");
  } finally {
    for (const dir of [sharedTarget, homeA, homeB]) {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  }
});

test("a missing binary freezes to nothing rather than throwing", () => {
  const home = scratch("missing");
  try {
    assert.equal(freezeArtifact(null, path.join(home, FROZEN_BIN_DIR)), null);
    assert.equal(freezeArtifact(path.join(home, "absent.exe"), path.join(home, FROZEN_BIN_DIR)), null);
    const run = freezeRunArtifacts({ home, appPath: null, cliPath: null });
    assert.equal(run.app, null);
    assert.equal(run.cli, null);
  } finally {
    fs.rmSync(home, { recursive: true, force: true });
  }
});
