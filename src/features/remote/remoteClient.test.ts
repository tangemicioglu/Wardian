import { afterEach, describe, expect, it, vi } from "vitest";
import { RemoteRequestError, remoteClient } from "./remoteClient";

describe("remoteClient error propagation", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    remoteClient.setCsrfNonce(null);
  });

  it("surfaces the gateway detail message when an action fails", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(
        new Response(
          JSON.stringify({
            ok: false,
            code: "agent_action_failed",
            detail: "Timed out waiting for ses_1 OpenCode terminal to become ready",
          }),
          { status: 400 },
        ),
      ),
    );

    const error = await remoteClient
      .sendPrompt("agent-1", "hello")
      .then(
        () => {
          throw new Error("expected rejection");
        },
        (cause: unknown) => cause as RemoteRequestError,
      );

    expect(error).toBeInstanceOf(RemoteRequestError);
    expect(error.status).toBe(400);
    expect(error.code).toBe("agent_action_failed");
    expect(error.detail).toBe("Timed out waiting for ses_1 OpenCode terminal to become ready");
    expect(error.message).toBe(
      "Remote request failed: Timed out waiting for ses_1 OpenCode terminal to become ready",
    );
  });

  it("keeps the status-only message when the error body has no detail", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(
        new Response(JSON.stringify({ ok: false, code: "agent_terminal_failed" }), {
          status: 400,
        }),
      ),
    );

    const error = await remoteClient
      .loadAgentTerminal("agent-1")
      .then(
        () => {
          throw new Error("expected rejection");
        },
        (cause: unknown) => cause as RemoteRequestError,
      );

    expect(error.message).toBe("Remote request failed: 400");
    expect(error.code).toBe("agent_terminal_failed");
    expect(error.detail).toBeUndefined();
  });
});
