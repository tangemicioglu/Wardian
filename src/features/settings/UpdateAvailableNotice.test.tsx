import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { UpdateAvailableNotice } from "./UpdateAvailableNotice";

describe("UpdateAvailableNotice", () => {
  it("offers a review action and a non-destructive later action", () => {
    const onReview = vi.fn();
    const onDismiss = vi.fn();

    render(
      <UpdateAvailableNotice
        update={{ version: "0.4.4" }}
        onDismiss={onDismiss}
        onReview={onReview}
      />,
    );

    expect(screen.getByRole("status")).toHaveTextContent("Wardian 0.4.4 is available.");
    fireEvent.click(screen.getByRole("button", { name: "Review update" }));
    expect(onReview).toHaveBeenCalledOnce();

    fireEvent.click(screen.getByRole("button", { name: "Dismiss update notice" }));
    expect(onDismiss).toHaveBeenCalledOnce();
  });
});
