import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { RemoteAutomationMonitorSnapshot } from "../../types";
import { RemoteAutomationsView } from "./RemoteAutomationsView";
import { remoteClient, RemoteRequestError } from "./remoteClient";

vi.mock("./remoteClient", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./remoteClient")>();
  return {
    ...actual,
    remoteClient: {
      ...actual.remoteClient,
      loadAutomationMonitor: vi.fn(),
    },
  };
});

const snapshot = (): RemoteAutomationMonitorSnapshot => ({
  schema_version: 1,
  generated_at: "2026-08-31T12:00:00.000Z",
  active_runs: [
    {
      run_id: "approval-1",
      blueprint_id: "release",
      automation_name: "Release validation",
      schedule_id: "schedule-1",
      status: "awaiting_approval",
      node_count: 4,
      completed_node_count: null,
      failure: null,
      started_at: "2026-08-31T11:40:00.000Z",
      updated_at: "2026-08-31T11:55:00.000Z",
      completed_at: null,
    },
    {
      run_id: "running-1",
      blueprint_id: "brief",
      automation_name: "Daily project brief",
      schedule_id: null,
      status: "running",
      node_count: 3,
      completed_node_count: null,
      failure: null,
      started_at: "2026-08-31T11:50:00.000Z",
      updated_at: "2026-08-31T11:59:00.000Z",
      completed_at: null,
    },
  ],
  active_runs_truncated: false,
  active_runs_next_offset: null,
  recent_runs: [
    {
      run_id: "completed-1",
      blueprint_id: "dependencies",
      automation_name: "Dependency refresh",
      schedule_id: null,
      status: "completed",
      node_count: 2,
      completed_node_count: null,
      failure: null,
      started_at: "2026-08-31T10:00:00.000Z",
      updated_at: "2026-08-31T10:05:00.000Z",
      completed_at: "2026-08-31T10:05:00.000Z",
    },
  ],
  recent_runs_truncated: true,
  recent_runs_next_offset: 25,
  schedules: [
    {
      id: "schedule-1",
      blueprint_id: "release",
      automation_name: "Morning status report",
      schedule: { schedule_type: "daily", time_of_day: "09:00", repeat_every: 1, end_condition: "never", occurrence_count: 0, active: true },
      next_run_epoch_ms: Date.parse("2026-09-01T13:00:00.000Z"),
      is_paused: false,
      last_run_status: "completed",
      last_run_error: null,
      last_run_epoch_ms: Date.parse("2026-08-31T13:00:00.000Z"),
      target_labels: ["writer · Agent"],
    },
  ],
  schedules_truncated: false,
  schedules_next_offset: null,
});

describe("RemoteAutomationsView", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(remoteClient.loadAutomationMonitor).mockResolvedValue(snapshot());
    window.history.replaceState({}, "", "/remote");
  });

  it("renders the glance-first hierarchy with touch-sized controls and accessible details", async () => {
    render(<RemoteAutomationsView />);

    expect(await screen.findByText("Needs attention")).toBeVisible();
    expect(screen.getByText("Running now")).toBeVisible();
    expect(screen.getByText("Up next")).toBeVisible();
    expect(screen.getByText("Recent outcomes")).toBeVisible();
    expect(screen.getByRole("button", { name: "Refresh automation monitor" })).toHaveClass("h-11", "w-11");
    expect(screen.getByRole("button", { name: "Overview" })).toHaveClass("min-h-11");

    await userEvent.click(screen.getByRole("button", { name: /Release validation/ }));
    expect(screen.getByRole("dialog", { name: "Release validation" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Close automation details" })).toHaveClass("h-11", "w-11");
  });

  it("keeps the successful snapshot visible and marks it stale when refresh fails", async () => {
    vi.mocked(remoteClient.loadAutomationMonitor)
      .mockResolvedValueOnce(snapshot())
      .mockRejectedValueOnce(new RemoteRequestError("unavailable", 503, "automation_monitor_unavailable"));
    render(<RemoteAutomationsView />);

    expect(await screen.findByText("Daily project brief")).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Refresh automation monitor" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("Showing saved results");
    expect(screen.getByText("Daily project brief")).toBeVisible();
  });

  it("merges a bounded history page by run identity", async () => {
    const next = snapshot();
    next.recent_runs = [
      snapshot().recent_runs[0],
      { ...snapshot().recent_runs[0], run_id: "completed-2", automation_name: "Weekly report" },
    ];
    next.recent_runs_truncated = false;
    next.recent_runs_next_offset = null;
    vi.mocked(remoteClient.loadAutomationMonitor)
      .mockResolvedValueOnce(snapshot())
      .mockResolvedValueOnce(next);
    render(<RemoteAutomationsView />);

    await userEvent.click(await screen.findByRole("button", { name: "History" }));
    await userEvent.click(screen.getByRole("button", { name: "Show older outcomes" }));

    expect(await screen.findByText("Weekly report")).toBeVisible();
    expect(screen.getAllByText("Dependency refresh")).toHaveLength(1);
    expect(screen.getByText("1 outcomes loaded.")).toBeInTheDocument();
    expect(remoteClient.loadAutomationMonitor).toHaveBeenLastCalledWith({ recent_offset: 25 });
  });

  it("coalesces repeated requests for the same pending history page", async () => {
    let resolvePage: ((value: RemoteAutomationMonitorSnapshot) => void) | undefined;
    const pendingPage = new Promise<RemoteAutomationMonitorSnapshot>((resolve) => {
      resolvePage = resolve;
    });
    vi.mocked(remoteClient.loadAutomationMonitor)
      .mockResolvedValueOnce(snapshot())
      .mockReturnValueOnce(pendingPage);
    render(<RemoteAutomationsView />);

    await userEvent.click(await screen.findByRole("button", { name: "History" }));
    const loadMore = screen.getByRole("button", { name: "Show older outcomes" });
    fireEvent.click(loadMore);
    fireEvent.click(loadMore);

    expect(remoteClient.loadAutomationMonitor).toHaveBeenCalledTimes(2);
    resolvePage?.(snapshot());
    await waitFor(() => expect(loadMore).not.toBeDisabled());
  });

  it("retains loaded history pages when a resume refresh replaces the first page", async () => {
    const olderPage = snapshot();
    olderPage.recent_runs = [
      { ...snapshot().recent_runs[0], run_id: "completed-2", automation_name: "Weekly report" },
    ];
    olderPage.recent_runs_truncated = false;
    olderPage.recent_runs_next_offset = null;
    const refreshed = snapshot();
    refreshed.recent_runs = [
      { ...snapshot().recent_runs[0], automation_name: "Dependency refresh updated" },
    ];
    vi.mocked(remoteClient.loadAutomationMonitor)
      .mockResolvedValueOnce(snapshot())
      .mockResolvedValueOnce(olderPage)
      .mockResolvedValueOnce(refreshed);
    render(<RemoteAutomationsView />);

    await userEvent.click(await screen.findByRole("button", { name: "History" }));
    await userEvent.click(screen.getByRole("button", { name: "Show older outcomes" }));
    expect(await screen.findByText("Weekly report")).toBeVisible();

    fireEvent(window, new Event("pageshow"));

    expect(await screen.findByText("Dependency refresh updated")).toBeVisible();
    expect(screen.getByText("Weekly report")).toBeVisible();
    expect(remoteClient.loadAutomationMonitor).toHaveBeenCalledTimes(3);
  });

  it("distinguishes an unsupported older desktop from a retryable failure", async () => {
    vi.mocked(remoteClient.loadAutomationMonitor).mockRejectedValue(
      new RemoteRequestError("missing", 404),
    );
    render(<RemoteAutomationsView />);

    expect(await screen.findByText("Automation monitoring is unavailable")).toBeVisible();
    expect(screen.getByText("Update the desktop app to use automation monitoring.")).toBeVisible();
    expect(screen.queryByRole("button", { name: "Retry" })).toBeNull();
  });

  it("refreshes when the page becomes visible again", async () => {
    render(<RemoteAutomationsView />);
    await screen.findByText("Daily project brief");
    fireEvent(window, new Event("pageshow"));

    await waitFor(() => expect(remoteClient.loadAutomationMonitor).toHaveBeenCalledTimes(2));
  });
});
