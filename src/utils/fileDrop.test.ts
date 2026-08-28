import { describe, expect, it } from "vitest";
import {
  decodeWardianFilePaths,
  encodeWardianFilePaths,
  formatDroppedPathsForTerminal,
  getDroppedFilePaths,
  isNativeFileDropInsideBounds,
  resolveTerminalShellId,
  setWardianFileDragData,
  WARDIAN_FILE_PATH_MIME,
  WARDIAN_FILE_PATHS_MIME,
} from "./fileDrop";

describe("file drop helpers", () => {
  it("encodes one path plainly and multiple paths as JSON", () => {
    expect(encodeWardianFilePaths(["C:\\repo\\notes.md"])).toBe("C:\\repo\\notes.md");
    expect(encodeWardianFilePaths(["/repo/a.txt", "/repo/b.txt"])).toBe("[\"/repo/a.txt\",\"/repo/b.txt\"]");
    expect(decodeWardianFilePaths("C:\\repo\\notes.md")).toEqual(["C:\\repo\\notes.md"]);
    expect(decodeWardianFilePaths("[\"/repo/a.txt\",\"/repo/b.txt\"]")).toEqual(["/repo/a.txt", "/repo/b.txt"]);
  });

  it("writes a custom payload that can cross the Wardian workbench", () => {
    const dataTransfer = {
      setData: (type: string, value: string) => values.set(type, value),
      effectAllowed: "",
    } as unknown as DataTransfer;
    const values = new Map<string, string>();

    setWardianFileDragData(dataTransfer, ["/repo/a.txt", "/repo/b.txt"]);

    expect(values.get(WARDIAN_FILE_PATH_MIME)).toBe("/repo/a.txt");
    expect(values.get(WARDIAN_FILE_PATHS_MIME)).toBe("[\"/repo/a.txt\",\"/repo/b.txt\"]");
    expect(dataTransfer.effectAllowed).toBe("copy");
  });

  it("reads Wardian paths, native file paths, and file URLs", () => {
    expect(getDroppedFilePaths({
      getData: (type) => type === WARDIAN_FILE_PATHS_MIME ? "[\"/repo/a.txt\",\"/repo/b.txt\"]" : "",
      files: [] as unknown as FileList,
    })).toEqual(["/repo/a.txt", "/repo/b.txt"]);

    const file = new File(["content"], "notes.md") as File & { path?: string };
    file.path = "C:\\repo\\notes.md";
    expect(getDroppedFilePaths({ getData: () => "", files: [file] as unknown as FileList })).toEqual(["C:\\repo\\notes.md"]);
    expect(getDroppedFilePaths({
      getData: (type) => type === "text/uri-list" ? "file:///C:/repo/notes%20draft.md" : "",
      files: [] as unknown as FileList,
    })).toEqual(["C:/repo/notes draft.md"]);
  });

  it("quotes paths safely for terminal insertion and leaves the prompt ready", () => {
    expect(formatDroppedPathsForTerminal(["C:\\repo\\notes draft.md", "/tmp/a.txt"], "powershell")).toBe("'C:\\repo\\notes draft.md' '/tmp/a.txt' ");
    expect(formatDroppedPathsForTerminal(["C:\\repo\\notes draft.md"], "cmd")).toBe('"C:\\repo\\notes draft.md" ');
    expect(formatDroppedPathsForTerminal(["C:\\repo\\$TEMP%HOME!draft^&.txt"], "powershell")).toBe("'C:\\repo\\$TEMP%HOME!draft^&.txt' ");
    expect(formatDroppedPathsForTerminal(["C:\\repo\\$TEMP%HOME!draft^&.txt"], "cmd")).toBe('"C:\\repo\\$TEMP^%HOME^!draft^^^&.txt" ');
    expect(formatDroppedPathsForTerminal(["/tmp/it's.txt"], "bash")).toBe("'/tmp/it'\\''s.txt' ");
    expect(formatDroppedPathsForTerminal(["C:\\repo\\notes draft.md"], "git-bash")).toBe("'/c/repo/notes draft.md' ");
    expect(formatDroppedPathsForTerminal([])).toBe("");
  });

  it("resolves automatic shells and physical native drop coordinates", () => {
    expect(resolveTerminalShellId("auto", ["cmd", "pwsh"], true)).toBe("pwsh");
    expect(resolveTerminalShellId("auto", ["bash"], false)).toBe("bash");
    expect(isNativeFileDropInsideBounds({ x: 300, y: 180 }, { left: 100, right: 250, top: 50, bottom: 150 }, 2)).toBe(true);
    expect(isNativeFileDropInsideBounds({ x: 502, y: 180 }, { left: 100, right: 250, top: 50, bottom: 150 }, 2)).toBe(false);
  });
});
