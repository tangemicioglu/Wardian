const TERMINAL_HOST_SELECTOR = '[data-testid="agent-terminal-host"]';

function presentationResolutionTimeoutMessage(scope, sessionId) {
  return `Timed out resolving the terminal presentation for ${scope} ${sessionId}`;
}

/** Selects one passive Agents card and resolves the app-level singleton host. */
export async function resolveAgentTerminalPresentationId(
  driver,
  sessionId,
  timeoutMs = 20_000,
) {
  return await driver.wait(async () => await driver.executeScript((sid, hostSelector) => {
    const card = document.getElementById(`agent-card-${sid}`);
    if (!card) return false;
    if (card.querySelector(hostSelector)) return false;
    const matchingHosts = [...document.querySelectorAll(
      `[data-zellij-singleton-viewport="true"] ${hostSelector}`,
    )].filter(
      (host) => host.getAttribute("data-terminal-session-id") === sid,
    );
    if (matchingHosts.length !== 1) {
      const preview = card.querySelector(
        `[data-zellij-presentation="preview"][data-zellij-agent-id="${CSS.escape(sid)}"]`,
      );
      if (preview instanceof HTMLElement && preview.getAttribute("aria-disabled") !== "true") {
        preview.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
      }
      return false;
    }
    const presentationId = matchingHosts[0].getAttribute("data-terminal-presentation-id");
    if (!presentationId) return false;
    const presentationIds = window.__wardianTerminalDebug?.presentationIds?.() ?? [];
    return presentationIds.includes(presentationId) ? presentationId : false;
  }, sessionId, TERMINAL_HOST_SELECTOR), timeoutMs,
  presentationResolutionTimeoutMessage("agent", sessionId));
}

/** Selects one agent-session preview and resolves the app-level singleton host. */
export async function resolveAgentSessionTerminalPresentationId(
  driver,
  sessionId,
  { surfaceId, timeoutMs = 20_000 } = {},
) {
  return await driver.wait(async () => await driver.executeScript(
    (sid, requestedSurfaceId, hostSelector) => {
      const panels = [...document.querySelectorAll(
        '[data-testid="surface-panel"][data-surface-type="agent-session"]',
      )].filter((panel) => (
        panel.getAttribute("data-resource-key") === sid &&
        (!requestedSurfaceId || panel.getAttribute("data-surface-id") === requestedSurfaceId)
      ));
      if (panels.length !== 1 || panels[0].querySelector(hostSelector)) return false;
      const matchingHosts = [...document.querySelectorAll(
        `[data-zellij-singleton-viewport="true"] ${hostSelector}`,
      )]
        .filter((host) => host.getAttribute("data-terminal-session-id") === sid);
      if (matchingHosts.length !== 1) {
        const preview = panels[0].querySelector(
          `[data-zellij-presentation="preview"][data-zellij-agent-id="${CSS.escape(sid)}"]`,
        );
        if (preview instanceof HTMLElement && preview.getAttribute("aria-disabled") !== "true") {
          preview.dispatchEvent(new PointerEvent("pointerdown", { bubbles: true }));
        }
        return false;
      }
      const presentationId = matchingHosts[0].getAttribute("data-terminal-presentation-id");
      if (!presentationId) return false;
      const presentationIds = window.__wardianTerminalDebug?.presentationIds?.() ?? [];
      return presentationIds.includes(presentationId) ? presentationId : false;
    },
    sessionId,
    surfaceId ?? null,
    TERMINAL_HOST_SELECTOR,
  ), timeoutMs, presentationResolutionTimeoutMessage("agent-session", sessionId));
}

export async function readTerminalDebugSnapshot(driver, presentationId) {
  return await driver.executeScript((pid) => (
    window.__wardianTerminalDebug?.snapshot(pid) ?? null
  ), presentationId);
}
