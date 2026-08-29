import { describe, expect, it } from "vitest";
import { isActiveAgentStatus, isActiveAutomationStatus, automationStatusColor } from "./gardenStatus";

describe("isActiveAgentStatus", () => {
  it("is true only for processing/headless work", () => {
    expect(isActiveAgentStatus("Processing")).toBe(true);
    expect(isActiveAgentStatus("headless")).toBe(true);
    expect(isActiveAgentStatus("Idle")).toBe(false);
    expect(isActiveAgentStatus("Off")).toBe(false);
    expect(isActiveAgentStatus("Action Needed")).toBe(false);
  });
});

describe("isActiveAutomationStatus", () => {
  it("is true only while a run is live or awaiting approval", () => {
    expect(isActiveAutomationStatus("running")).toBe(true);
    expect(isActiveAutomationStatus("awaiting_approval")).toBe(true);
    expect(isActiveAutomationStatus("completed")).toBe(false);
    expect(isActiveAutomationStatus("failed")).toBe(false);
    expect(isActiveAutomationStatus("none")).toBe(false);
  });
});

describe("automationStatusColor", () => {
  it("maps each run status to a theme variable", () => {
    expect(automationStatusColor("running")).toBe("var(--color-wardian-processing)");
    expect(automationStatusColor("awaiting_approval")).toBe("var(--color-wardian-warning)");
    expect(automationStatusColor("completed")).toBe("var(--color-wardian-success)");
    expect(automationStatusColor("failed")).toBe("var(--color-wardian-error)");
    expect(automationStatusColor("none")).toBe("var(--color-wardian-text-muted)");
  });
});
