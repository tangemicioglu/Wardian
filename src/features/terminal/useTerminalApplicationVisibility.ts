import { useEffect } from "react";
import { setTerminalApplicationVisibility } from "./terminalSessionClient";

/**
 * Reports whether the WebView document is actually hidden to terminal broker
 * clients.
 *
 * Window focus is deliberately not part of this signal. An unfocused Wardian
 * window can remain visible on another monitor, and native focus notifications
 * are not guaranteed to pair cleanly across operating-system window changes.
 * Treating blur as hidden can therefore leave a visible terminal paused after
 * the user returns to it.
 *
 * Workbench surface visibility deliberately does not participate: a hidden
 * resident tab is still part of the active app and keeps its normal stream.
 */
export function useTerminalApplicationVisibility() {
  useEffect(() => {
    const publish = () => {
      void setTerminalApplicationVisibility(document.visibilityState !== "hidden");
    };

    document.addEventListener("visibilitychange", publish);
    publish();

    return () => {
      document.removeEventListener("visibilitychange", publish);
    };
  }, []);
}
