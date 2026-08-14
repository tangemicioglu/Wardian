import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { WorkbenchGroupHeader } from "./WorkbenchGroupHeader";

describe("WorkbenchGroupHeader", () => {
  it("keeps an open-tabs menu available and activates its selected tab", () => {
    const onActivateSurface = vi.fn();
    render(
      <WorkbenchGroupHeader
        group_id="group-1"
        active_surface_id="file-2"
        tabs={[
          { surface_id: "file-1", title: "architecture.md" },
          { surface_id: "file-2", title: "README.md" },
        ]}
        on_activate_surface={onActivateSurface}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Show open tabs" }));

    const menu = screen.getByRole("menu", { name: "Open tabs" });
    expect(menu).toBeVisible();
    expect(screen.getByRole("menuitem", { name: "README.md" }))
      .toHaveAttribute("aria-current", "page");

    fireEvent.click(screen.getByRole("menuitem", { name: "architecture.md" }));
    expect(onActivateSurface).toHaveBeenCalledWith("group-1", "file-1");
  });
});
