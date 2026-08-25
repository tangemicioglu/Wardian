import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";

import {
  createNativeHarness,
  ensureNativeAppBuilt,
  prepareIsolatedHome,
  startNativeSession,
  waitForAppShell,
} from "../lib/harness.mjs";
import {
  closeWorkbenchSurface,
  waitForWorkbenchReady,
} from "../lib/workbench.mjs";

const skipNativeBuild = process.env.WARDIAN_NATIVE_SKIP_BUILD === "1";
const RUN_ID = `${process.pid}-${Date.now()}`;
const PROVIDER_SESSION_ID = `e2e-terminal-broker-${RUN_ID}`;
const SESSION_NAME = `E2E-Terminal-Broker-${RUN_ID}`;
let wardianSessionId = null;
const OWNER_GEOMETRY = Object.freeze({ cols: 101, rows: 31 });
const MIRROR_GEOMETRY = Object.freeze({ cols: 151, rows: 44 });
const ZELLIJ_CANONICAL_GEOMETRY = Object.freeze({ cols: 120, rows: 40 });

async function invokeTauri(driver, command, args = {}) {
  const result = await driver.executeAsyncScript((cmd, payload, done) => {
    window.__TAURI_INTERNALS__.invoke(cmd, payload).then(
      (value) => done({ ok: true, value }),
      (error) => done({ ok: false, error: String(error) }),
    );
  }, command, args);

  assert.equal(result.ok, true, `${command} failed: ${result.error}`);
  return result.value;
}

async function waitFor(label, timeoutMs, probe) {
  const startedAt = Date.now();
  let last = null;
  while (Date.now() - startedAt < timeoutMs) {
    try {
      last = await probe();
      if (last?.ok) return last;
    } catch (error) {
      last = { ok: false, error: String(error) };
    }
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  throw new Error(`Timed out waiting for ${label}: ${JSON.stringify(last)}`);
}

function writeBrokerMockScript(harness) {
  const scriptPath = path.join(harness.isolatedHome, `terminal-broker-${RUN_ID}.cjs`);
  fs.writeFileSync(
    scriptPath,
    `
"use strict";
const readline = require("node:readline");
let tick = 0;
process.stdout.write(JSON.stringify({
  type: "init",
  session_id: ${JSON.stringify(PROVIDER_SESSION_ID)},
  timestamp: new Date().toISOString(),
}) + "\\n");
process.stdout.write("BROKER_READY_${RUN_ID}\\r\\n");
setInterval(() => {
  tick += 1;
  process.stdout.write("broker-tick:" + tick + "\\r\\n");
}, 100);
const lines = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
lines.on("line", (line) => process.stdout.write("broker-echo:" + line + "\\r\\n"));
process.stdin.resume();
`,
    "utf8",
  );
  return scriptPath;
}

async function updatePresentation(
  driver,
  presentationId,
  runtimeGeneration,
  desiredGeometry,
) {
  return await invokeTauri(driver, "update_terminal_presentation", {
    request: {
      presentation_id: presentationId,
      session_id: wardianSessionId,
      runtime_generation: runtimeGeneration,
      desired_geometry: desiredGeometry,
      visibility: "visible",
      render_state: "mounted",
      requested_interaction: "interactive",
      observed_lease_epoch: 0,
    },
  });
}

async function registerPresentation(driver, presentationId, desiredGeometry) {
  return await invokeTauri(driver, "register_terminal_presentation", {
    request: {
      presentation_id: presentationId,
      session_id: wardianSessionId,
      client_kind: "desktop",
      desired_geometry: desiredGeometry,
      visibility: "visible",
      render_state: "mounted",
      requested_interaction: "interactive",
      observed_lease_epoch: 0,
    },
  });
}

async function beginActivation(driver, presentationId, brokerState) {
  return await invokeTauri(driver, "begin_terminal_activation", {
    request: {
      session_id: wardianSessionId,
      presentation_id: presentationId,
      runtime_generation: brokerState.runtime_generation,
      observed_lease_epoch: brokerState.lease_epoch,
    },
  });
}

async function ackActivation(driver, presentationId, begin) {
  return await invokeTauri(driver, "ack_terminal_activation", {
    request: {
      session_id: wardianSessionId,
      presentation_id: presentationId,
      runtime_generation: begin.decision.runtime_generation,
      lease_epoch: begin.decision.lease_epoch,
      activation_id: begin.activation_id,
    },
  });
}

async function resizePresentation(
  driver,
  presentationId,
  brokerState,
  geometrySequence,
  geometry,
) {
  return await invokeTauri(driver, "resize_terminal_presentation", {
    request: {
      session_id: wardianSessionId,
      presentation_id: presentationId,
      runtime_generation: brokerState.runtime_generation,
      lease_epoch: brokerState.lease_epoch,
      geometry_sequence: geometrySequence,
      cols: geometry.cols,
      rows: geometry.rows,
    },
  });
}

async function reportPresentationViewport(driver, presentationId, runtimeGeneration, geometry) {
  return await invokeTauri(driver, "report_terminal_presentation_viewport", {
    request: {
      session_id: wardianSessionId,
      presentation_id: presentationId,
      runtime_generation: runtimeGeneration,
      cols: geometry.cols,
      rows: geometry.rows,
    },
  });
}

async function drainBrokerEvents(driver, consumerId, runtimeGeneration, afterSequence) {
  const events = [];
  let cursor = afterSequence;
  for (let batchIndex = 0; batchIndex < 32; batchIndex += 1) {
    const batch = await invokeTauri(driver, "read_terminal_events", {
      request: {
        session_id: wardianSessionId,
        consumer_id: consumerId,
        runtime_generation: runtimeGeneration,
        after_sequence: cursor,
        max_events: 256,
        max_bytes: 256 * 1024,
      },
    });
    assert.equal(batch.status, "events", `unexpected broker recovery batch: ${JSON.stringify(batch)}`);
    events.push(...batch.events);
    cursor = batch.next_sequence;
    if (cursor >= batch.latest_sequence) {
      return { cursor, events };
    }
  }
  assert.fail(`terminal broker drain did not catch up after 32 batches; cursor=${cursor}`);
}

function outputText(event) {
  return event.type === "output" ? Buffer.from(event.bytes ?? []).toString("utf8") : "";
}

function assertRecoveryWindow(
  events,
  activationId,
  ownerPresentationId,
  label,
) {
  const pendingIndex = events.findIndex(
    (event) => event.type === "ownership"
      && event.activation_id === activationId
      && event.owner_presentation_id === null,
  );
  assert.ok(pendingIndex >= 0, `${label}: pending ownership event is missing`);
  const rollbackIndex = events.findIndex(
    (event, index) => index > pendingIndex
      && event.type === "ownership"
      && event.activation_id === null
      && event.owner_presentation_id === ownerPresentationId,
  );
  assert.ok(rollbackIndex > pendingIndex, `${label}: fallback ownership event is missing`);
  assert.equal(
    events.slice(pendingIndex + 1, rollbackIndex).some((event) => event.type === "output"),
    true,
    `${label}: broker output did not continue while activation was pending`,
  );
}

test(
  "desktop terminal presentations keep explicit ownership and ordered broker recovery",
  { timeout: 300000 },
  async (t) => {
    const harness = await createNativeHarness();
    prepareIsolatedHome(harness);
    const mockScript = writeBrokerMockScript(harness);
    const previousMockScript = process.env.WARDIAN_MOCK_SCRIPT;
    process.env.WARDIAN_MOCK_SCRIPT = mockScript;

    let session = null;
    try {
      if (!skipNativeBuild) ensureNativeAppBuilt(harness);
      assert.ok(harness.appPath, "Expected a native Wardian application path");
      session = await startNativeSession(harness);
    } finally {
      if (previousMockScript === undefined) delete process.env.WARDIAN_MOCK_SCRIPT;
      else process.env.WARDIAN_MOCK_SCRIPT = previousMockScript;
    }

    t.after(async () => {
      await session?.close();
      fs.rmSync(mockScript, { force: true });
    });

    const { driver } = session;
    await waitForAppShell(driver, 20000);
    await waitForWorkbenchReady(driver);
    await driver.manage().window().setRect({ width: 1600, height: 900 });

    const agent = await invokeTauri(driver, "spawn_agent", {
      req: {
        sessionName: SESSION_NAME,
        agentClass: "TestClass",
        folder: harness.repoRoot,
        resumeSession: PROVIDER_SESSION_ID,
        isOff: false,
        configOverride: { provider: "mock" },
      },
    });
    wardianSessionId = agent.session_id;
    assert.match(
      wardianSessionId,
      /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i,
    );
    assert.notEqual(wardianSessionId, PROVIDER_SESSION_ID);

    // Agents intentionally claims an otherwise unowned runtime so its first
    // terminal paint is interactive without a click. Close that presentation
    // before this broker test establishes its own explicit owner/mirror pair.
    await closeWorkbenchSurface(driver, "agents-overview");

    const ownerPresentationId = `native-owner:${wardianSessionId}`;
    const mirrorPresentationId = `native-mirror:${wardianSessionId}`;

    const terminalSnapshot = await invokeTauri(driver, "request_terminal_snapshot", {
      request: { session_id: wardianSessionId },
    });
    const runtimeGeneration = terminalSnapshot.runtime_generation;
    const owner = await registerPresentation(driver, ownerPresentationId, OWNER_GEOMETRY);
    const mirror = await registerPresentation(driver, mirrorPresentationId, MIRROR_GEOMETRY);
    const registered = { owner, mirror };
    assert.equal(registered.mirror.broker_state.owner_presentation_id, null);
    assert.equal(registered.mirror.broker_state.pending_activation, null);

    const passiveFocusState = await updatePresentation(
      driver,
      ownerPresentationId,
      runtimeGeneration,
      OWNER_GEOMETRY,
    );
    assert.equal(passiveFocusState.broker_state.owner_presentation_id, null);
    assert.equal(passiveFocusState.broker_state.pending_activation, null);

    const consumerId = `desktop:${wardianSessionId}`;
    await invokeTauri(driver, "subscribe_terminal_events", {
      request: {
        session_id: wardianSessionId,
        consumer_id: consumerId,
        client_kind: "desktop",
        runtime_generation: runtimeGeneration,
      },
    });
    const eventStartSnapshot = await invokeTauri(driver, "request_terminal_snapshot", {
      request: { session_id: wardianSessionId },
    });
    await waitFor("shared desktop broker feed", 30000, async () => {
      const batch = await invokeTauri(driver, "read_terminal_events", {
        request: {
          session_id: wardianSessionId,
          consumer_id: consumerId,
          runtime_generation: runtimeGeneration,
          after_sequence: eventStartSnapshot.sequence_barrier,
          max_events: 1,
          max_bytes: 1024,
        },
      });
      return { ok: batch.status === "events", batch };
    });
    let brokerState = passiveFocusState.broker_state;

    const initialBegin = await beginActivation(driver, ownerPresentationId, brokerState);
    assert.equal(initialBegin.decision.status, "accepted");
    assert.ok(initialBegin.activation_id);
    assert.ok(initialBegin.snapshot);
    const initialPending = await updatePresentation(
      driver,
      mirrorPresentationId,
      runtimeGeneration,
      MIRROR_GEOMETRY,
    );
    assert.equal(initialPending.broker_state.owner_presentation_id, null);
    assert.equal(
      initialPending.broker_state.pending_activation?.presentation_id,
      ownerPresentationId,
    );
    const initialAck = await ackActivation(driver, ownerPresentationId, initialBegin);
    assert.equal(initialAck.decision.status, "accepted");
    assert.equal(initialAck.broker_state.owner_presentation_id, ownerPresentationId);
    assert.equal(initialAck.broker_state.pending_activation, null);
    brokerState = initialAck.broker_state;

    const ownerCanonical = await resizePresentation(
      driver,
      ownerPresentationId,
      brokerState,
      10_000,
      OWNER_GEOMETRY,
    );
    assert.equal(ownerCanonical.decision.status, "rejected");
    assert.equal(ownerCanonical.decision.reason, "fixed_geometry");
    assert.deepEqual(ownerCanonical.geometry, ZELLIJ_CANONICAL_GEOMETRY);
    brokerState = {
      ...brokerState,
      geometry: ownerCanonical.geometry,
      lease_epoch: ownerCanonical.decision.lease_epoch,
    };
    const mirrorViewport = await invokeTauri(driver, "report_terminal_presentation_viewport", {
      request: {
        session_id: wardianSessionId,
        presentation_id: mirrorPresentationId,
        runtime_generation: runtimeGeneration,
        cols: 220,
        rows: 70,
      },
    });
    assert.deepEqual(mirrorViewport.desired_geometry, { cols: 220, rows: 70 });
    const mirrorInput = await invokeTauri(driver, "send_terminal_presentation_input", {
      request: {
        session_id: wardianSessionId,
        presentation_id: mirrorPresentationId,
        runtime_generation: runtimeGeneration,
        lease_epoch: brokerState.lease_epoch,
        input: `MIRROR_MUST_NOT_WRITE_${RUN_ID}\r`,
      },
    });
    assert.equal(mirrorInput.status, "rejected");
    assert.equal(mirrorInput.reason, "not_owner");
    const mirrorResize = await resizePresentation(
      driver,
      mirrorPresentationId,
      brokerState,
      1,
      { cols: 230, rows: 75 },
    );
    assert.equal(mirrorResize.decision.status, "rejected");
    assert.equal(mirrorResize.decision.reason, "not_owner");
    assert.deepEqual(mirrorResize.geometry, ZELLIJ_CANONICAL_GEOMETRY);
    const stableAfterMirror = await updatePresentation(
      driver,
      ownerPresentationId,
      runtimeGeneration,
      OWNER_GEOMETRY,
    );
    assert.deepEqual(stableAfterMirror.broker_state.geometry, ZELLIJ_CANONICAL_GEOMETRY);
    brokerState = stableAfterMirror.broker_state;

    const ownerInputMarker = `OWNER_INPUT_${RUN_ID}`;
    const ownerInput = await invokeTauri(driver, "send_terminal_presentation_input", {
      request: {
        session_id: wardianSessionId,
        presentation_id: ownerPresentationId,
        runtime_generation: runtimeGeneration,
        lease_epoch: brokerState.lease_epoch,
        input: `${ownerInputMarker}\r`,
      },
    });
    assert.equal(ownerInput.status, "accepted");

    await reportPresentationViewport(
      driver,
      ownerPresentationId,
      runtimeGeneration,
      OWNER_GEOMETRY,
    );
    const raceGeometry = await resizePresentation(
      driver,
      ownerPresentationId,
      brokerState,
      20_000,
      { cols: 104, rows: 32 },
    );
    assert.equal(raceGeometry.decision.status, "rejected");
    assert.equal(raceGeometry.decision.reason, "fixed_geometry");
    assert.deepEqual(raceGeometry.geometry, ZELLIJ_CANONICAL_GEOMETRY);
    brokerState = { ...brokerState, geometry: raceGeometry.geometry };
    const raceMirrorBegin = await beginActivation(driver, mirrorPresentationId, brokerState);
    assert.equal(raceMirrorBegin.decision.status, "accepted");
    await new Promise((resolve) => setTimeout(resolve, 300));
    const raceOwnerBegin = await beginActivation(driver, ownerPresentationId, {
      ...brokerState,
      lease_epoch: raceMirrorBegin.decision.lease_epoch,
    });
    assert.equal(raceOwnerBegin.decision.status, "accepted");
    assert.notEqual(raceOwnerBegin.activation_id, raceMirrorBegin.activation_id);
    assert.ok(raceOwnerBegin.snapshot, "race activation must capture canonical geometry");
    const raceExpectedGeometry = raceOwnerBegin.snapshot.geometry;
    assert.deepEqual(raceExpectedGeometry, ZELLIJ_CANONICAL_GEOMETRY);
    await new Promise((resolve) => setTimeout(resolve, 300));
    const staleRaceAck = await ackActivation(driver, mirrorPresentationId, raceMirrorBegin);
    assert.equal(staleRaceAck.decision.status, "rejected");
    assert.equal(staleRaceAck.decision.reason, "stale_activation");
    const raceOwnerAck = await ackActivation(driver, ownerPresentationId, raceOwnerBegin);
    assert.equal(raceOwnerAck.decision.status, "accepted");
    assert.equal(raceOwnerAck.broker_state.owner_presentation_id, ownerPresentationId);
    // The newly committed UI owner immediately reports its locally fitted
    // viewport, so the response may already be newer than the activation
    // snapshot. Event-order assertions below still prove snapshot restoration
    // preceded the ownership commit.
    assert.deepEqual(raceOwnerAck.broker_state.geometry, ZELLIJ_CANONICAL_GEOMETRY);
    brokerState = raceOwnerAck.broker_state;

    await reportPresentationViewport(
      driver,
      ownerPresentationId,
      runtimeGeneration,
      OWNER_GEOMETRY,
    );
    const timeoutGeometry = await resizePresentation(
      driver,
      ownerPresentationId,
      brokerState,
      30_000,
      { cols: 108, rows: 33 },
    );
    assert.equal(timeoutGeometry.decision.status, "rejected");
    assert.equal(timeoutGeometry.decision.reason, "fixed_geometry");
    assert.deepEqual(timeoutGeometry.geometry, ZELLIJ_CANONICAL_GEOMETRY);
    brokerState = { ...brokerState, geometry: timeoutGeometry.geometry };
    const timeoutBegin = await beginActivation(driver, mirrorPresentationId, brokerState);
    assert.equal(timeoutBegin.decision.status, "accepted");
    assert.ok(timeoutBegin.snapshot, "timeout activation must capture canonical geometry");
    const timeoutExpectedGeometry = timeoutBegin.snapshot.geometry;
    const pendingOwnerInput = await invokeTauri(driver, "send_terminal_presentation_input", {
      request: {
        session_id: wardianSessionId,
        presentation_id: ownerPresentationId,
        runtime_generation: runtimeGeneration,
        lease_epoch: timeoutBegin.decision.lease_epoch,
        input: `PENDING_MUST_NOT_WRITE_${RUN_ID}\r`,
      },
    });
    assert.equal(pendingOwnerInput.status, "rejected");
    assert.equal(pendingOwnerInput.reason, "pending_activation");
    const timeoutFallback = await waitFor("activation timeout rollback", 15000, async () => {
      const value = await updatePresentation(
        driver,
        ownerPresentationId,
        runtimeGeneration,
        timeoutExpectedGeometry,
      );
      return {
        ok: value.broker_state.owner_presentation_id === ownerPresentationId
          && value.broker_state.pending_activation === null,
        value,
      };
    });
    assert.deepEqual(
      timeoutFallback.value.broker_state.geometry,
      ZELLIJ_CANONICAL_GEOMETRY,
    );
    brokerState = timeoutFallback.value.broker_state;

    const disconnectGeometry = await resizePresentation(
      driver,
      ownerPresentationId,
      brokerState,
      40_000,
      { cols: 112, rows: 35 },
    );
    assert.equal(disconnectGeometry.decision.status, "rejected");
    assert.equal(disconnectGeometry.decision.reason, "fixed_geometry");
    assert.deepEqual(disconnectGeometry.geometry, ZELLIJ_CANONICAL_GEOMETRY);
    brokerState = { ...brokerState, geometry: disconnectGeometry.geometry };
    const disconnectBegin = await beginActivation(driver, mirrorPresentationId, brokerState);
    assert.equal(disconnectBegin.decision.status, "accepted");
    assert.ok(disconnectBegin.snapshot, "disconnect activation must capture canonical geometry");
    const disconnectExpectedGeometry = disconnectBegin.snapshot.geometry;
    assert.deepEqual(disconnectExpectedGeometry, ZELLIJ_CANONICAL_GEOMETRY);
    await new Promise((resolve) => setTimeout(resolve, 400));
    await invokeTauri(driver, "unregister_terminal_presentation", {
      request: {
        session_id: wardianSessionId,
        presentation_id: mirrorPresentationId,
        runtime_generation: runtimeGeneration,
      },
    });
    const disconnectFallback = await waitFor("disconnect rollback", 30000, async () => {
      const value = await updatePresentation(
        driver,
        ownerPresentationId,
        runtimeGeneration,
        disconnectExpectedGeometry,
      );
      return {
        ok: value.broker_state.owner_presentation_id === ownerPresentationId
          && value.broker_state.pending_activation === null,
        value,
      };
    });
    const disconnectFallbackGeometry = disconnectFallback.value.broker_state.geometry;
    assert.deepEqual(disconnectFallbackGeometry, ZELLIJ_CANONICAL_GEOMETRY);

    await new Promise((resolve) => setTimeout(resolve, 300));
    const drained = await drainBrokerEvents(
      driver,
      consumerId,
      runtimeGeneration,
      eventStartSnapshot.sequence_barrier,
    );
    assert.ok(drained.events.length > 0, "Expected broker events after the snapshot barrier");
    for (let index = 0; index < drained.events.length; index += 1) {
      assert.equal(
        drained.events[index].sequence,
        eventStartSnapshot.sequence_barrier + index + 1,
        `broker cursor gap at event index ${index}`,
      );
    }
    assert.equal(
      drained.events.some((event) => outputText(event).includes(`broker-echo:${ownerInputMarker}`)),
      true,
      "owner input did not reach the mock PTY output stream",
    );
    assert.equal(
      drained.events.some((event) => outputText(event).includes("MIRROR_MUST_NOT_WRITE")),
      false,
      "mirror input reached the mock PTY despite a rejected lease decision",
    );

    const racePendingIndex = drained.events.findIndex(
      (event) => event.type === "ownership"
        && event.activation_id === raceMirrorBegin.activation_id
        && event.owner_presentation_id === null,
    );
    const raceCommitIndex = drained.events.findIndex(
      (event, index) => index > racePendingIndex
        && event.type === "ownership"
        && event.activation_id === raceOwnerBegin.activation_id
        && event.owner_presentation_id === ownerPresentationId,
    );
    assert.ok(racePendingIndex >= 0 && raceCommitIndex > racePendingIndex);
    assertRecoveryWindow(
      drained.events,
      timeoutBegin.activation_id,
      ownerPresentationId,
      "activation timeout",
    );
    assertRecoveryWindow(
      drained.events,
      disconnectBegin.activation_id,
      ownerPresentationId,
      "presentation disconnect",
    );
  },
);
