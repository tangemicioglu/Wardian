import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { WorkbenchNavigationService } from "../workbench/navigationService";
import { DEFAULT_FILE_OPEN_ACTIONS } from "../../types/settings";
import {
  fileOpenDestinationForPath,
  fileOpenKindForPath,
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

describe("file open routing", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockInvoke.mockResolvedValue(undefined);
  });

  it("classifies supported files by broad renderer family", () => {
    expect(fileOpenKindForPath("C:/repo/App.tsx")).toBe("text");
    expect(fileOpenKindForPath("C:/repo/diagram.png")).toBe("image");
    expect(fileOpenKindForPath("C:/repo/report.pdf")).toBe("pdf");
    expect(fileOpenKindForPath("C:/repo/report.docx")).toBeNull();
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
    expect(mockInvoke).not.toHaveBeenCalled();
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
    expect(fileOpenDestinationForPath("C:/repo/report.docx", { text: "wardian" })).toBe("system");

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
});
