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

function props(on_open_system: FileRendererProps["on_open_system"]): FileRendererProps {
  return {
    snapshot: snapshot(),
    client: {} as FileRendererProps["client"],
    lifecycle: { visible: true },
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
    expect(screen.queryByText("Preview unavailable")).not.toBeInTheDocument();
  });

  it("keeps the recovery actions when launching the system viewer fails", async () => {
    const on_open_system = vi.fn().mockRejectedValue(new Error("no associated app"));

    render(<UnsupportedRenderer {...props(on_open_system)} />);

    expect(await screen.findByRole("alert")).toHaveTextContent("no associated app");
    expect(screen.getByRole("button", { name: "Open With" })).toBeInTheDocument();
  });
});
