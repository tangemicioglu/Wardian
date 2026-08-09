import type { FileRendererKind } from "../types";

/** Returns a minimal descriptor response for tests that mock Tauri invoke. */
export function mockOpenFileResource(
  command: string,
  args?: unknown,
): { subscription_id: string; descriptor: { renderer_kind: FileRendererKind } } | null {
  if (command !== "open_file_resource") return null;
  const path = typeof args === "object" && args !== null
    && "request" in args && typeof args.request === "object" && args.request !== null
    && "path" in args.request && typeof args.request.path === "string"
    ? args.request.path.toLowerCase()
    : "";
  const renderer_kind: FileRendererKind =
    /\.(bmp|docx|tiff)$/.test(path)
      ? "unsupported"
      : path.endsWith(".pdf")
        ? "pdf"
        : /\.(gif|jpe?g|png|webp)$/.test(path)
          ? "image"
          : path.endsWith(".md") || path.endsWith(".markdown")
            ? "markdown"
            : "text";
  return {
    subscription_id: "file-open-test-subscription",
    descriptor: { renderer_kind },
  };
}
