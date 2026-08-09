import { render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { FileResourceSnapshotV1 } from "../../types";
import type { FileRendererProps } from "./rendererRegistry";
import UnsupportedRenderer from "./UnsupportedRenderer";

function snapshot(): FileResourceSnapshotV1 {
  return {
    resource_id: "file:C:/repo/report.docx",
    subscription_id: "subscription-1",
    revision: 1,
    descriptor: {
      schema: 1,
      canonical_path: "C:/repo/report.docx",
      display_name: "report.docx",
      extension: "docx",
      mime_type: "application/octet-stream",
      encoding: null,
      renderer_kind: "unsupported",
      size_bytes: 1024,
      line_count: null,
      content_hash: "bounded-sha256:test",
      modified_at_ms: 1,
      capabilities: { preview: false, changes: false, draft: false, stream: false },
      unavailable_reason: "unsupported_content",
    },
  };
}

function props(
  on_open_system: FileRendererProps["on_open_system"],
  currentSnapshot = snapshot(),
  visible = true,
): FileRendererProps {
  return {
    snapshot: currentSnapshot,
    client: {} as FileRendererProps["client"],
    lifecycle: { visible },
    on_open_file: vi.fn(),
    on_open_with: vi.fn(),
    on_open_system,
    on_reveal: vi.fn(),
  };
}

describe("UnsupportedRenderer", () => {
  it("opens unsupported content in the system viewer instead of leaving the empty fallback", async () => {
    const on_open_system = vi.fn().mockResolvedValue(undefined);

    render(<UnsupportedRenderer {...props(on_open_system)} />);

    expect(screen.getByRole("heading", { name: "Opening in system viewer" })).toBeInTheDocument();
    await waitFor(() => expect(on_open_system).toHaveBeenCalledWith("C:/repo/report.docx"));
    expect(await screen.findByRole("heading", { name: "Opened in system viewer" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Open With" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Opening in system viewer" })).not.toBeInTheDocument();
  });

  it("keeps the recovery actions when launching the system viewer fails", async () => {
    const on_open_system = vi.fn().mockRejectedValue(new Error("no associated app"));

    render(<UnsupportedRenderer {...props(on_open_system)} />);

    expect(await screen.findByRole("alert")).toHaveTextContent("no associated app");
    expect(screen.getByRole("button", { name: "Open With" })).toBeInTheDocument();
  });

  it("resets the launch state when a later resource revision succeeds", async () => {
    const on_open_system = vi.fn()
      .mockRejectedValueOnce(new Error("no associated app"))
      .mockResolvedValueOnce(undefined);
    const view = render(<UnsupportedRenderer {...props(on_open_system)} />);

    expect(await screen.findByRole("alert")).toHaveTextContent("no associated app");

    const nextSnapshot = snapshot();
    nextSnapshot.revision = 2;
    view.rerender(<UnsupportedRenderer {...props(on_open_system, nextSnapshot)} />);

    expect(await screen.findByRole("heading", { name: "Opened in system viewer" })).toBeInTheDocument();
    expect(screen.queryByText(/no associated app/)).not.toBeInTheDocument();
    expect(on_open_system).toHaveBeenCalledTimes(2);
  });

  it("preserves a settled launch state when visibility changes", async () => {
    const on_open_system = vi.fn().mockResolvedValue(undefined);
    const view = render(<UnsupportedRenderer {...props(on_open_system)} />);

    expect(await screen.findByRole("heading", { name: "Opened in system viewer" })).toBeInTheDocument();

    view.rerender(<UnsupportedRenderer {...props(on_open_system, snapshot(), false)} />);
    view.rerender(<UnsupportedRenderer {...props(on_open_system)} />);

    expect(screen.getByRole("heading", { name: "Opened in system viewer" })).toBeInTheDocument();
    expect(on_open_system).toHaveBeenCalledTimes(1);
  });
});
