import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { WorkbenchNavigationService } from "../workbench/navigationService";
import { DEFAULT_FILE_OPEN_ACTIONS } from "../../types/settings";
import type { FileResourceSnapshotV1 } from "../../types";
import { mockOpenFileResource } from "../../test/fileResourceMock";
import {
  fileOpenDestinationForKind,
  fileOpenKindForRendererKind,
  openFileWithSettings,
} from "./fileOpenRouting";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const mockInvoke = vi.mocked(invoke);

function navigation() {
  return {
    open: vi.fn(() => "surface-1"),
    pin_transient: vi.fn(),
  } as unknown as WorkbenchNavigationService;
}

function verifiedSnapshot(path: string, renderer_kind: FileResourceSnapshotV1["descriptor"]["renderer_kind"]): FileResourceSnapshotV1 {
  return {
    resource_id: `file:${path}`,
    subscription_id: "subscription-1",
    revision: 1,
    descriptor: {
      schema: 1,
      canonical_path: path,
      display_name: path.split(/[\\/]/).pop() ?? path,
      extension: null,
      mime_type: renderer_kind === "image" ? "image/png" : renderer_kind === "pdf" ? "application/pdf" : "text/plain",
      encoding: renderer_kind === "unsupported" || renderer_kind === "image" || renderer_kind === "pdf" ? null : "UTF-8",
      renderer_kind,
      size_bytes: 1,
      line_count: renderer_kind === "text" || renderer_kind === "markdown" ? 1 : null,
      content_hash: "sha256:test",
      modified_at_ms: 1,
      capabilities: { preview: true, changes: false, draft: false, stream: false },
      unavailable_reason: null,
    },
  };
}

describe("file open routing", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockInvoke.mockImplementation((command, args) => {
      const fileResource = mockOpenFileResource(command, args);
      if (fileResource) return Promise.resolve(fileResource);
      return Promise.resolve(undefined);
    });
  });

  it("classifies supported files by broad renderer family", () => {
    expect(fileOpenKindForRendererKind("text")).toBe("text");
    expect(fileOpenKindForRendererKind("markdown")).toBe("text");
    expect(fileOpenKindForRendererKind("image")).toBe("image");
    expect(fileOpenKindForRendererKind("pdf")).toBe("pdf");
    expect(fileOpenKindForRendererKind("unsupported")).toBeNull();
    expect(fileOpenDestinationForKind("text", { text: "wardian" })).toBe("wardian");
    expect(fileOpenDestinationForKind(null, { text: "wardian" })).toBe("system");
  });

  it("routes Markdown files through the verified text-family preference", async () => {
    const nav = navigation();

    await openFileWithSettings("C:/repo/README.markdown", {
      navigation: nav,
      file_open_actions: { text: "wardian" },
    });

    expect(nav.open).toHaveBeenCalledWith(expect.objectContaining({
      surface_type: "files",
      resource_key: "file:C:/repo/README.markdown",
    }));

    await openFileWithSettings("C:/repo/README.markdown", {
      file_open_actions: { text: "external" },
      external_editor: "vscode",
    });

    expect(mockInvoke).toHaveBeenCalledWith("open_in_external_editor", {
      path: "C:/repo/README.markdown",
      editor: {
        external_editor: "vscode",
        external_editor_custom_executable: null,
      },
    });
  });

  it.each([
    ["asset.bin", "image" as const, "wardian" as const],
    ["document.data", "pdf" as const, "external" as const],
    ["source.png", "text" as const, "wardian" as const],
  ])("uses the verified renderer family for %s", async (name, renderer_kind, action) => {
    mockInvoke.mockImplementation((command) => {
      if (command === "open_file_resource") {
        return Promise.resolve(verifiedSnapshot(`C:/repo/${name}`, renderer_kind));
      }
      return Promise.resolve(undefined);
    });

    const nav = navigation();
    const result = await openFileWithSettings(`C:/repo/${name}`, {
      navigation: nav,
      file_open_actions: {
        text: "external",
        image: "external",
        pdf: "external",
        [renderer_kind === "text" ? "text" : renderer_kind]: action,
      },
      external_editor: "vscode",
    });

    if (action === "wardian") {
      expect(result).toBe("wardian");
      expect(nav.open).toHaveBeenCalled();
      expect(mockInvoke).toHaveBeenCalledWith("close_file_resource", {
        request: { subscription_id: "subscription-1" },
      });
    } else {
      expect(result).toBe("external");
      expect(mockInvoke).toHaveBeenCalledWith("open_in_external_editor", expect.objectContaining({
        path: `C:/repo/${name}`,
      }));
    }
  });

  it("opens Wardian-preferred supported links as permanent Files surfaces", async () => {
    const nav = navigation();

    await openFileWithSettings("C:/repo/App.tsx", {
      navigation: nav,
      file_open_actions: DEFAULT_FILE_OPEN_ACTIONS,
      external_editor: "vscode",
    });

    expect(nav.open).toHaveBeenCalledWith(expect.objectContaining({
      surface_type: "files",
      resource_key: "file:C:/repo/App.tsx",
    }));
    expect(nav.pin_transient).toHaveBeenCalledWith("surface-1");
    expect(mockInvoke).toHaveBeenCalledWith("open_file_resource", {
      request: {
        path: "C:/repo/App.tsx",
        agent_id: null,
        user_file_capability_id: null,
      },
    });
    expect(mockInvoke).not.toHaveBeenCalledWith("open_in_external_editor", expect.anything());
  });

  it("uses the configured external editor when a supported type is external-preferred", async () => {
    await openFileWithSettings("C:/repo/App.tsx", {
      file_open_actions: { text: "external" },
      external_editor: "custom",
      external_editor_custom_executable: "C:/Tools/editor.exe",
    });

    expect(mockInvoke).toHaveBeenCalledWith("open_in_external_editor", {
      path: "C:/repo/App.tsx",
      editor: {
        external_editor: "custom",
        external_editor_custom_executable: "C:/Tools/editor.exe",
      },
    });
  });

  it("always sends unsupported content to the system preferred viewer", async () => {
    await openFileWithSettings("C:/repo/report.docx", {
      file_open_actions: { text: "wardian" },
      external_editor: "custom",
      external_editor_custom_executable: "C:/Tools/editor.exe",
    });

    expect(mockInvoke).toHaveBeenCalledWith("open_in_external_editor", {
      path: "C:/repo/report.docx",
      editor: {
        external_editor: "system",
        external_editor_custom_executable: null,
      },
    });
  });

  it("uses the system fallback when the content descriptor cannot be verified", async () => {
    mockInvoke.mockImplementation((command) => (
      command === "open_file_resource"
        ? Promise.reject(new Error("descriptor unavailable"))
        : Promise.resolve(undefined)
    ));

    const result = await openFileWithSettings("C:/repo/looks-like-image.png", {
      file_open_actions: { image: "wardian" },
      external_editor: "vscode",
    });

    expect(result).toBe("system");
    expect(mockInvoke).toHaveBeenCalledWith("open_in_external_editor", {
      path: "C:/repo/looks-like-image.png",
      editor: {
        external_editor: "system",
        external_editor_custom_executable: null,
      },
    });
  });
});
