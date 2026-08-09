import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { QueueItem } from "../../types";
import { RemoteInboxView } from "./RemoteInboxView";
import { useRemoteStore } from "./useRemoteStore";

const originalOpenAgent = useRemoteStore.getState().openAgent;

afterEach(() => {
  cleanup();
  useRemoteStore.setState({ remoteQueueItems: [], openAgent: originalOpenAgent });
});

describe("RemoteInboxView", () => {
  it("collapses long summaries and opens the related agent", () => {
    const openAgent = vi.fn().mockResolvedValue(undefined);
    const item: QueueItem = {
      id: "remote-inbox-1",
      type: "agent_update",
      timestamp: Date.now(),
      read: false,
      agent_session_id: "agent-1",
      notification_title: "Agent task completed",
      summary: "A long update that contains enough detail to need a collapsed preview.\n\n"
        + "The mobile card should start in the same compact state as the desktop Inbox card, "
        + "then reveal the complete message when the user asks for more details.",
    };
    useRemoteStore.setState({ remoteQueueItems: [item], openAgent });

    render(<RemoteInboxView />);

    const summary = screen.getByTestId("remote-queue-item-summary-remote-inbox-1");
    const toggle = screen.getByRole("button", { name: "Show full summary" });
    expect(screen.getByText("Agent task completed")).toBeVisible();
    expect(screen.getByTestId("remote-queue-unread-dot")).toBeVisible();
    expect(summary).toHaveClass("line-clamp-4");
    expect(toggle).toHaveAttribute("aria-expanded", "false");

    fireEvent.click(toggle);

    expect(summary).toHaveClass("max-h-80");
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("button", { name: "Collapse summary" })).toBeVisible();

    fireEvent.click(screen.getByRole("button", { name: "Open agent terminal" }));
    expect(openAgent).toHaveBeenCalledWith("agent-1");
  });
});
