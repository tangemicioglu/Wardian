export const WARDIAN_FILE_PATH_MIME = "text/x-wardian-file-path";
export const WARDIAN_FILE_PATHS_MIME = "text/x-wardian-file-paths";

type FileWithPath = File & { path?: string };

export interface NativeFileDropPosition {
  x?: number;
  y?: number;
}

export interface FileDropBounds {
  left: number;
  right: number;
  top: number;
  bottom: number;
}

/** Encodes a drag payload while keeping the common single-file case readable. */
export function encodeWardianFilePaths(paths: readonly string[]): string {
  return paths.length === 1 ? paths[0] : JSON.stringify(paths);
}

/** Decodes a Wardian drag payload and ignores malformed multi-path values. */
export function decodeWardianFilePaths(value: string): string[] {
  if (!value) return [];
  try {
    const parsed: unknown = JSON.parse(value);
    if (Array.isArray(parsed)) {
      return parsed.filter((path): path is string => typeof path === "string" && path.trim().length > 0);
    }
  } catch {
    // A single path is intentionally stored without JSON wrapping.
  }
  return value.trim() ? [value] : [];
}

/** Places the absolute paths of Explorer files on a browser drag operation. */
export function setWardianFileDragData(dataTransfer: DataTransfer, paths: readonly string[]): void {
  const validPaths = paths.filter((path) => path.trim().length > 0);
  if (validPaths.length === 0) return;

  dataTransfer.setData(WARDIAN_FILE_PATH_MIME, validPaths[0]);
  dataTransfer.setData(WARDIAN_FILE_PATHS_MIME, encodeWardianFilePaths(validPaths));
  dataTransfer.effectAllowed = "copy";
}

/** Returns whether a drag operation represents files we can accept. */
export function hasWardianFileDropData(dataTransfer: Pick<DataTransfer, "types">): boolean {
  return dataTransfer.types.includes("Files")
    || dataTransfer.types.includes(WARDIAN_FILE_PATH_MIME)
    || dataTransfer.types.includes(WARDIAN_FILE_PATHS_MIME);
}

/** Extracts filesystem paths from Wardian, native Tauri, or browser file drops. */
export function getDroppedFilePaths(
  dataTransfer: Pick<DataTransfer, "getData"> & Partial<Pick<DataTransfer, "files">>,
): string[] {
  const internalPaths = dataTransfer.getData(WARDIAN_FILE_PATHS_MIME)
    || dataTransfer.getData(WARDIAN_FILE_PATH_MIME);
  if (internalPaths) return decodeWardianFilePaths(internalPaths);

  const files = Array.from(dataTransfer.files ?? []) as FileWithPath[];
  const filePaths = files
    .map((file) => file.path)
    .filter((path): path is string => Boolean(path));
  if (filePaths.length > 0) return filePaths;

  return (dataTransfer.getData("text/uri-list") || "")
    .split(/\r?\n/)
    .filter((value) => value.startsWith("file://"))
    .map(fileUrlToPath)
    .filter((path): path is string => Boolean(path));
}

/** Hit-tests a native drop after converting physical coordinates to CSS pixels. */
export function isNativeFileDropInsideBounds(
  position: NativeFileDropPosition,
  bounds: FileDropBounds,
  deviceScale = 1,
): boolean {
  const scale = deviceScale > 0 ? deviceScale : 1;
  const x = (position.x ?? -1) / scale;
  const y = (position.y ?? -1) / scale;
  return x >= bounds.left && x <= bounds.right && y >= bounds.top && y <= bounds.bottom;
}

/** Gets the display name used for a dropped attachment chip. */
export function fileNameFromPath(path: string): string {
  const normalized = path.replace(/\\/g, "/");
  const segments = normalized.split("/").filter(Boolean);
  return segments[segments.length - 1] || path;
}

/** Formats paths for insertion without submitting the terminal prompt. */
/** Resolves the configured shell, including the platform's automatic choice. */
export function resolveTerminalShellId(
  shellId: string,
  availableShellIds: readonly string[] = [],
  isWindows = false,
): string {
  if (shellId !== "auto") return shellId;
  const preferred = isWindows
    ? ["pwsh", "powershell", "cmd", "git-bash", "wsl", "bash"]
    : ["zsh", "bash", "sh", "fish"];
  return preferred.find((id) => availableShellIds.includes(id)) ?? preferred[0];
}

/** Formats paths for the configured shell without submitting the terminal prompt. */
export function formatDroppedPathsForTerminal(
  paths: readonly string[],
  shellId = "auto",
): string {
  const formatted = paths
    .filter((path) => path.trim().length > 0)
    .map((path) => escapeTerminalPath(pathForTerminalShell(path, shellId), shellId))
    .join(" ");
  return formatted ? `${formatted} ` : "";
}

function escapeTerminalPath(path: string, shellId: string): string {
  if (shellId === "powershell" || shellId === "pwsh") {
    return `'${path.replace(/'/g, "''")}'`;
  }
  if (shellId === "cmd") {
    const escaped = path.replace(/["^&|<>()]/g, "^$&").replace(/%/g, "^%").replace(/!/g, "^!");
    return /^[A-Za-z0-9_./\\:@+-]+$/.test(path) ? path : `"${escaped.replace(/"/g, '\\"')}"`;
  }
  if (/^[A-Za-z0-9_./@:-]+$/.test(path)) return path;
  return `'${path.replace(/'/g, "'\\''")}'`;
}

function pathForTerminalShell(path: string, shellId: string): string {
  if (!/^[A-Za-z]:[\\/]/.test(path)) return path;
  if (shellId !== "bash" && shellId !== "git-bash" && shellId !== "wsl") return path;
  const normalized = path.replace(/\\/g, "/");
  const drive = normalized[0].toLowerCase();
  const rest = normalized.slice(2).replace(/^\/+/, "");
  return `/${shellId === "wsl" ? "mnt/" : ""}${drive}/${rest}`;
}

function fileUrlToPath(value: string): string | null {
  try {
    const url = new URL(value);
    let path = decodeURIComponent(url.pathname);
    if (/^\/[A-Za-z]:\//.test(path)) path = path.slice(1);
    if (url.hostname && url.hostname !== "localhost") path = `//${url.hostname}${path}`;
    return path;
  } catch {
    return null;
  }
}
