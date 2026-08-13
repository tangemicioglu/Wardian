import { memo } from "react";
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { keepHiddenSurfaceSnapshot } from "./hiddenSurfaceMemo";

describe("hidden surface memoization", () => {
  it("defers parent updates while hidden and catches up on reveal", () => {
    const rendered = vi.fn();
    const Probe = memo(function Probe({
      value,
      visibility,
    }: {
      value: string;
      visibility: "visible" | "hidden";
    }) {
      rendered(value);
      return <span>{visibility}:{value}</span>;
    }, keepHiddenSurfaceSnapshot);

    const view = render(<Probe value="first" visibility="hidden" />);
    view.rerender(<Probe value="second" visibility="hidden" />);

    expect(screen.getByText("hidden:first")).toBeInTheDocument();
    expect(rendered).toHaveBeenCalledTimes(1);

    view.rerender(<Probe value="second" visibility="visible" />);

    expect(screen.getByText("visible:second")).toBeInTheDocument();
    expect(rendered).toHaveBeenCalledTimes(2);
  });

  it("does not suppress updates while visible or across a hide transition", () => {
    expect(keepHiddenSurfaceSnapshot<{ visibility: "visible" | "hidden" }>(
      { visibility: "visible" },
      { visibility: "visible" },
    )).toBe(false);
    expect(keepHiddenSurfaceSnapshot<{ visibility: "visible" | "hidden" }>(
      { visibility: "visible" },
      { visibility: "hidden" },
    )).toBe(false);
  });
});
