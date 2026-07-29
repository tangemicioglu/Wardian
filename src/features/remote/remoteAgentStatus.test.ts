import { describe, expect, it } from "vitest";
import { remoteStatusClassFor } from "./remoteAgentStatus";

describe("remoteStatusClassFor", () => {
  it("uses the shared agent-status tones for provider variants", () => {
    expect(remoteStatusClassFor("Processing...")).toBe("bg-wardian-processing");
    expect(remoteStatusClassFor("Action Required")).toBe("bg-wardian-warning");
    expect(remoteStatusClassFor("failed")).toBe("bg-wardian-error");
    expect(remoteStatusClassFor("offline")).toBe("bg-wardian-off");
  });
});
