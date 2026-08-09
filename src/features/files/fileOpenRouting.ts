import { invoke } from "@tauri-apps/api/core";
import type { WorkbenchNavigationService } from "../workbench/navigationService";
import { DEFAULT_FILE_OPEN_ACTIONS, type ExternalEditorSetting, type FileOpenActions, type FileOpenKind } from "../../types/settings";
import { openPermanentFileSurface } from "./fileSurfaceNavigation";

export type FileOpenDestination = "wardian" | "external" | "system";

export type FileOpenRoutingOptions = {
  navigation?: WorkbenchNavigationService | null;
  file_open_actions?: Partial<FileOpenActions> | null;
  external_editor?: ExternalEditorSetting;
  external_editor_custom_executable?: string | null;
};

const TEXT_EXTENSIONS = new Set([
  "bat", "c", "cc", "cjs", "cmd", "conf", "cpp", "cs", "css", "csv", "diff", "env", "go", "h", "hpp",
  "htm", "html", "ini", "java", "js", "json", "jsx", "lock", "log", "md", "mdx", "mjs", "patch", "ps1",
  "py", "rs", "scss", "sh", "sql", "svelte", "svg", "toml", "ts", "tsx", "txt", "vue", "xml", "yaml", "yml", "markdown",
]);
const IMAGE_EXTENSIONS = new Set(["bmp", "gif", "jpg", "jpeg", "png", "tif", "tiff", "webp"]);

function fileNameFromPath(path: string) {
  const withoutLocation = path.replace(/:\d+(?::\d+)?$/, "");
  return withoutLocation.split(/[\\/]/).filter(Boolean).pop()?.toLowerCase() ?? "";
}

/** Classifies only the broad families Wardian can currently render. */
export function fileOpenKindForPath(path: string): FileOpenKind | null {
  const fileName = fileNameFromPath(path);
  if (fileName === "dockerfile" || fileName === "makefile" || fileName.startsWith(".env")) {
    return "text";
  }
  const extension = fileName.includes(".") ? fileName.split(".").pop() ?? "" : "";
  if (extension === "pdf") return "pdf";
  if (IMAGE_EXTENSIONS.has(extension)) return "image";
  if (TEXT_EXTENSIONS.has(extension)) return "text";
  return null;
}

export function fileOpenDestinationForPath(
  path: string,
  actions: Partial<FileOpenActions> | null | undefined = DEFAULT_FILE_OPEN_ACTIONS,
): FileOpenDestination {
  const kind = fileOpenKindForPath(path);
  if (!kind) return "system";
  return actions?.[kind] === "wardian" ? "wardian" : "external";
}

export async function openFileInExternalApp(
  path: string,
  editor: {
    external_editor: ExternalEditorSetting;
    external_editor_custom_executable?: string | null;
  },
) {
  await invoke("open_in_external_editor", {
    path,
    editor: {
      external_editor: editor.external_editor,
      external_editor_custom_executable: editor.external_editor_custom_executable?.trim() || null,
    },
  });
}

/** Opens a link according to the file-type preference, with safe fallbacks. */
export async function openFileWithSettings(path: string, options: FileOpenRoutingOptions) {
  const destination = fileOpenDestinationForPath(path, options.file_open_actions);
  if (destination === "wardian" && options.navigation) {
    openPermanentFileSurface(options.navigation, path);
    return destination;
  }

  await openFileInExternalApp(path, {
    external_editor: destination === "system" ? "system" : (options.external_editor ?? "system"),
    external_editor_custom_executable: destination === "system"
      ? null
      : options.external_editor_custom_executable,
  });
  return destination === "wardian" ? "external" : destination;
}
