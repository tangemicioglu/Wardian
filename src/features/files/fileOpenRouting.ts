import { invoke } from "@tauri-apps/api/core";
import type { WorkbenchNavigationService } from "../workbench/navigationService";
import { DEFAULT_FILE_OPEN_ACTIONS, type ExternalEditorSetting, type FileOpenActions, type FileOpenKind } from "../../types/settings";
import type { FileContentDescriptorV1, FileRendererKind, OpenFileResourceRequestV1 } from "../../types";
import { openPermanentFileSurface } from "./fileSurfaceNavigation";
import { fileResourceClient } from "./fileResourceClient";

export type FileOpenDestination = "wardian" | "external" | "system";

export type FileOpenRoutingOptions = {
  navigation?: WorkbenchNavigationService | null;
  file_open_actions?: Partial<FileOpenActions> | null;
  external_editor?: ExternalEditorSetting;
  external_editor_custom_executable?: string | null;
  resource_request?: OpenFileResourceRequestV1 | null;
  verified_renderer_kind?: FileRendererKind;
};

/** Maps the backend's verified renderer family to the settings family. */
export function fileOpenKindForRendererKind(
  rendererKind: FileRendererKind | null | undefined,
): FileOpenKind | null {
  switch (rendererKind) {
    case "text":
    case "markdown":
      return "text";
    case "image":
      return "image";
    case "pdf":
      return "pdf";
    default:
      return null;
  }
}

export function fileOpenKindForDescriptor(
  descriptor: Pick<FileContentDescriptorV1, "renderer_kind">,
): FileOpenKind | null {
  return fileOpenKindForRendererKind(descriptor.renderer_kind);
}

export function fileOpenDestinationForKind(
  kind: FileOpenKind | null,
  actions: Partial<FileOpenActions> | null | undefined = DEFAULT_FILE_OPEN_ACTIONS,
): FileOpenDestination {
  if (!kind) return "system";
  return actions?.[kind] === "wardian" ? "wardian" : "external";
}

async function verifiedFileOpenKind(
  path: string,
  resourceRequest?: OpenFileResourceRequestV1 | null,
): Promise<FileOpenKind | null> {
  const request: OpenFileResourceRequestV1 = {
    ...(resourceRequest ?? {
      agent_id: null,
      user_file_capability_id: null,
    }),
    path,
  };
  try {
    const snapshot = await fileResourceClient.open(request);
    if (!snapshot?.descriptor) {
      if (resourceRequest) throw new Error("The Files backend returned no file descriptor.");
      return null;
    }
    try {
      return fileOpenKindForDescriptor(snapshot.descriptor);
    } finally {
      await fileResourceClient.close(snapshot.subscription_id).catch(() => undefined);
    }
  } catch (error) {
    // An unverified resource must use the system fallback, never a filename
    // hint that could select the wrong family preference. Explicit
    // authorization failures still propagate so they cannot become launches.
    if (resourceRequest) throw error;
    return null;
  }
}

export async function fileOpenDestinationForResource(
  path: string,
  options: Pick<
    FileOpenRoutingOptions,
    "file_open_actions" | "resource_request" | "verified_renderer_kind"
  > = {},
): Promise<FileOpenDestination> {
  const kind = options.verified_renderer_kind === undefined
    ? await verifiedFileOpenKind(path, options.resource_request)
    : fileOpenKindForRendererKind(options.verified_renderer_kind);
  return fileOpenDestinationForKind(kind, options.file_open_actions);
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
  const destination = await fileOpenDestinationForResource(path, options);
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
