import { useEffect } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { setTerminalApplicationVisibility } from "./terminalSessionClient";

/**
 * Reports actual desktop-window backgrounding to terminal broker clients.
 * Workbench surface visibility deliberately does not participate: a hidden
 * resident tab is still part of the active app and keeps its normal stream.
 */
export function useTerminalApplicationVisibility() {
  useEffect(() => {
    let disposed = false;
    let nativeFocused: boolean | null = null;
    let documentVisible = document.visibilityState !== "hidden";
    let browserFocused = document.hasFocus();
    let unlisten: (() => void) | undefined;

    const publish = () => {
      const focused = nativeFocused ?? browserFocused;
      void setTerminalApplicationVisibility(documentVisible && focused);
    };
    const onVisibilityChange = () => {
      documentVisible = document.visibilityState !== "hidden";
      publish();
    };
    const onFocus = () => {
      browserFocused = true;
      publish();
    };
    const onBlur = () => {
      browserFocused = false;
      publish();
    };

    document.addEventListener("visibilitychange", onVisibilityChange);
    window.addEventListener("focus", onFocus);
    window.addEventListener("blur", onBlur);
    publish();

    try {
      getCurrentWindow().onFocusChanged((event) => {
        nativeFocused = event.payload;
        publish();
      }).then((cleanup) => {
        if (disposed) {
          cleanup();
          return;
        }
        unlisten = cleanup;
      }).catch((error) => {
        console.warn("Failed to listen for native terminal visibility", error);
      });
    } catch (error) {
      console.warn("Failed to listen for native terminal visibility", error);
    }

    return () => {
      disposed = true;
      unlisten?.();
      document.removeEventListener("visibilitychange", onVisibilityChange);
      window.removeEventListener("focus", onFocus);
      window.removeEventListener("blur", onBlur);
    };
  }, []);
}
