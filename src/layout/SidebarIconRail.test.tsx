import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { SidebarIconRail } from "./SidebarIconRail";

describe("SidebarIconRail density", () => {
  it("uses original-size icons inside roomy activity rail slots", () => {
    render(
      <SidebarIconRail
        activeTab="explorer"
        setActiveTab={vi.fn()}
        setCollapsed={vi.fn()}
        userTerminalOpen={false}
        settingsOpen={false}
        onToggleUserTerminal={vi.fn()}
        onToggleSettings={vi.fn()}
      />,
    );

    expect(screen.getByTestId("sidebar-icon-rail")).toHaveClass("gap-3");
    const explorerIcon = screen.getByTestId("sidebar-tab-explorer").querySelector("svg");
    expect(screen.getByTestId("sidebar-tab-explorer")).toHaveClass("p-3");
    expect(explorerIcon).toHaveClass("w-6");
    expect(explorerIcon).toHaveClass("h-6");
    expect(screen.getByTestId("sidebar-tab-changes")).toHaveAttribute("title", "Changes");
    const titles = screen.getAllByRole("button").map((button) => button.getAttribute("title"));
    expect(titles.indexOf("File Explorer")).toBe(0);
    expect(titles.indexOf("Agent Configuration")).toBe(1);
    expect(titles.indexOf("Agent Configuration")).toBeLessThan(titles.indexOf("Source Control"));
  });

  it("opens Agent Configuration when its stable rail target is selected", async () => {
    const user = userEvent.setup();
    const setActiveTab = vi.fn();
    const setCollapsed = vi.fn();
    render(
      <SidebarIconRail
        activeTab="explorer"
        setActiveTab={setActiveTab}
        setCollapsed={setCollapsed}
        userTerminalOpen={false}
        settingsOpen={false}
        onToggleUserTerminal={vi.fn()}
        onToggleSettings={vi.fn()}
      />,
    );

    await user.click(screen.getByTestId("sidebar-tab-agent-config"));

    expect(setActiveTab).toHaveBeenCalledWith("agent-config");
    expect(setCollapsed).toHaveBeenCalledWith(false);
  });

  it("does not reserve a persistent help slot on the icon rail", () => {
    render(
      <SidebarIconRail
        activeTab="explorer"
        setActiveTab={vi.fn()}
        setCollapsed={vi.fn()}
        userTerminalOpen={false}
        settingsOpen={false}
        onToggleUserTerminal={vi.fn()}
        onToggleSettings={vi.fn()}
      />,
    );

    expect(screen.queryByTestId("sidebar-help-getting-started")).not.toBeInTheDocument();
  });

  it("does not expose remote connections on the icon rail", () => {
    render(
      <SidebarIconRail
        activeTab="explorer"
        setActiveTab={vi.fn()}
        setCollapsed={vi.fn()}
        userTerminalOpen={false}
        settingsOpen={false}
        onToggleUserTerminal={vi.fn()}
        onToggleSettings={vi.fn()}
      />,
    );

    expect(screen.queryByTitle("Remote Connections")).not.toBeInTheDocument();
  });

  it("shows a source control pending-change badge only when changes exist", () => {
    const { rerender } = render(
      <SidebarIconRail
        activeTab="explorer"
        setActiveTab={vi.fn()}
        setCollapsed={vi.fn()}
        userTerminalOpen={false}
        settingsOpen={false}
        sourceControlChangeCount={12}
        onToggleUserTerminal={vi.fn()}
        onToggleSettings={vi.fn()}
      />,
    );

    expect(screen.getByTestId("sidebar-tab-git-badge")).toHaveTextContent("12");
    expect(screen.getByTestId("sidebar-tab-git-badge")).toHaveAttribute("aria-label", "12 pending source control changes");

    rerender(
      <SidebarIconRail
        activeTab="explorer"
        setActiveTab={vi.fn()}
        setCollapsed={vi.fn()}
        userTerminalOpen={false}
        settingsOpen={false}
        sourceControlChangeCount={0}
        onToggleUserTerminal={vi.fn()}
        onToggleSettings={vi.fn()}
      />,
    );

    expect(screen.queryByTestId("sidebar-tab-git-badge")).not.toBeInTheDocument();
  });

  it("shows a source control progress marker while git is refreshing", () => {
    render(
      <SidebarIconRail
        activeTab="explorer"
        setActiveTab={vi.fn()}
        setCollapsed={vi.fn()}
        userTerminalOpen={false}
        settingsOpen={false}
        sourceControlBusy={true}
        onToggleUserTerminal={vi.fn()}
        onToggleSettings={vi.fn()}
      />,
    );

    expect(screen.getByTestId("sidebar-tab-git-progress")).toHaveAttribute(
      "aria-label",
      "Source control is refreshing",
    );
  });

  it("marks Settings when an update is available", () => {
    render(
      <SidebarIconRail
        activeTab="explorer"
        setActiveTab={vi.fn()}
        setCollapsed={vi.fn()}
        userTerminalOpen={false}
        settingsOpen={false}
        updateAvailable
        onToggleUserTerminal={vi.fn()}
        onToggleSettings={vi.fn()}
      />,
    );

    expect(screen.getByTestId("sidebar-tab-settings-update-badge")).toHaveAttribute(
      "aria-label",
      "Update available",
    );
    expect(screen.getByTestId("sidebar-tab-settings")).toHaveAttribute(
      "aria-label",
      "Application Settings (update available)",
    );
  });
});
