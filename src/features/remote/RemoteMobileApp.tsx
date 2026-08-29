import React, { lazy, Suspense, useEffect, useRef, useState } from "react";
import "../../styles/App.css";
import { RefreshCw } from "lucide-react";
import { RemoteBottomNav } from "./RemoteBottomNav";
import { RemoteWatchlistView } from "./RemoteWatchlistView";
import { useRemoteStore } from "./useRemoteStore";
import { useSettingsStore } from "../../store/useSettingsStore";

const RemoteAgentDetailView = lazy(() =>
  import("./RemoteAgentDetailView").then((module) => ({ default: module.RemoteAgentDetailView }))
);
const RemoteInboxView = lazy(() =>
  import("./RemoteInboxView").then((module) => ({ default: module.RemoteInboxView }))
);
const RemotePairingView = lazy(() =>
  import("./RemotePairingView").then((module) => ({ default: module.RemotePairingView }))
);
const RemoteSettingsView = lazy(() =>
  import("./RemoteSettingsView").then((module) => ({ default: module.RemoteSettingsView }))
);

const REMOTE_HISTORY_SETTINGS_VIEW = "settings";

const currentHistoryStateObject = () =>
  typeof window.history.state === "object" && window.history.state !== null && !Array.isArray(window.history.state)
    ? window.history.state
    : {};

const isRemoteSettingsHistoryState = (state = window.history.state) =>
  typeof state === "object" &&
  state !== null &&
  !Array.isArray(state) &&
  (state as { wardian_remote_view?: unknown }).wardian_remote_view === REMOTE_HISTORY_SETTINGS_VIEW;

const resolveTheme = (theme: "system" | "light" | "dark") => {
  if (theme !== "system") return theme;
  return window.matchMedia?.("(prefers-color-scheme: light)")?.matches ? "light" : "dark";
};

const RemoteSurfaceFallback: React.FC = () => (
  <main className="flex min-h-screen items-center justify-center bg-wardian-bg p-4 text-primary">
    <div className="inline-flex items-center gap-2 text-sm text-muted-neutral">
      <RefreshCw className="h-4 w-4 animate-spin" aria-hidden="true" />
      Loading Wardian...
    </div>
  </main>
);

export const RemoteMobileApp: React.FC = () => {
  const agents = useRemoteStore((state) => state.agents);
  const status = useRemoteStore((state) => state.status);
  const activeAgentId = useRemoteStore((state) => state.activeAgentId);
  const activeRemoteTab = useRemoteStore((state) => state.activeRemoteTab);
  const theme = useSettingsStore((state) => state.theme);
  const load = useRemoteStore((state) => state.load);
  const refresh = useRemoteStore((state) => state.refresh);
  const disconnectStatusStream = useRemoteStore((state) => state.disconnectStatusStream);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const settingsOpenRef = useRef(false);
  const resumeRefreshInFlightRef = useRef(false);

  useEffect(() => {
    settingsOpenRef.current = settingsOpen;
  }, [settingsOpen]);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    return () => disconnectStatusStream();
  }, [disconnectStatusStream]);

  useEffect(() => {
    const closeOverlayOnHistoryBack = () => {
      if (settingsOpenRef.current) {
        setSettingsOpen(false);
        return;
      }
      const state = useRemoteStore.getState();
      if (state.activeAgentId) state.closeAgent({ syncHistory: false });
    };

    window.addEventListener("popstate", closeOverlayOnHistoryBack);
    return () => {
      window.removeEventListener("popstate", closeOverlayOnHistoryBack);
    };
  }, []);

  useEffect(() => {
    const mediaQuery = window.matchMedia?.("(prefers-color-scheme: light)");
    const applyTheme = () => {
      document.documentElement.setAttribute("data-theme", resolveTheme(theme));
    };
    applyTheme();
    mediaQuery?.addEventListener?.("change", applyTheme);
    return () => mediaQuery?.removeEventListener?.("change", applyTheme);
  }, [theme]);

  useEffect(() => {
    const refreshOnResume = () => {
      if (document.visibilityState === "hidden") return;
      if (useRemoteStore.getState().status !== "ready") return;
      if (resumeRefreshInFlightRef.current) return;
      resumeRefreshInFlightRef.current = true;
      void refresh().finally(() => {
        resumeRefreshInFlightRef.current = false;
      });
    };

    window.addEventListener("focus", refreshOnResume);
    window.addEventListener("pageshow", refreshOnResume);
    document.addEventListener("visibilitychange", refreshOnResume);
    return () => {
      window.removeEventListener("focus", refreshOnResume);
      window.removeEventListener("pageshow", refreshOnResume);
      document.removeEventListener("visibilitychange", refreshOnResume);
    };
  }, [refresh]);

  if (status === "loading") {
    return (
      <main className="flex min-h-screen items-center justify-center bg-wardian-bg p-4 text-primary">
        <div className="inline-flex items-center gap-2 text-sm text-muted-neutral">
          <RefreshCw className="h-4 w-4 animate-spin" aria-hidden="true" />
          Loading Wardian...
        </div>
      </main>
    );
  }

  let pairingView: React.ReactNode = null;
  if (status === "session_expired") {
    pairingView = (
      <RemotePairingView state="session_expired" actionLabel="Re-authenticate" onAction={() => void load()} />
    );
  } else if (status === "pairing_pending") {
    pairingView = <RemotePairingView state="pending" />;
  } else if (status === "pairing_expired") {
    pairingView = <RemotePairingView state="expired" />;
  } else if (status === "gateway_identity_changed") {
    pairingView = <RemotePairingView state="identity_changed" />;
  } else if (status === "device_revoked") {
    pairingView = <RemotePairingView state="revoked" />;
  } else if (status === "unreachable") {
    pairingView = <RemotePairingView state="unreachable" actionLabel="Retry" onAction={() => void load()} />;
  }

  if (pairingView) {
    return <Suspense fallback={<RemoteSurfaceFallback />}>{pairingView}</Suspense>;
  }

  const activeAgent = agents.find((agent) => agent.session_id === activeAgentId);
  if (activeAgent) {
    return (
      <Suspense fallback={<RemoteSurfaceFallback />}>
        <RemoteAgentDetailView agent={activeAgent} />
      </Suspense>
    );
  }

  const openSettings = () => {
    setSettingsOpen(true);
    try {
      window.history.pushState(
        {
          ...currentHistoryStateObject(),
          wardian_remote_view: REMOTE_HISTORY_SETTINGS_VIEW,
        },
        "",
        `${window.location.pathname}${window.location.search}${window.location.hash}`,
      );
    } catch {
      // Some embedded browsers restrict history writes; the in-app Back button remains available.
    }
  };

  const closeSettings = () => {
    if (isRemoteSettingsHistoryState()) {
      try {
        window.history.back();
        setSettingsOpen(false);
        return;
      } catch {
        // If browser history cannot move, still close the in-app settings view.
      }
    }
    setSettingsOpen(false);
  };

  if (settingsOpen) {
    return (
      <Suspense fallback={<RemoteSurfaceFallback />}>
        <RemoteSettingsView onClose={closeSettings} />
      </Suspense>
    );
  }

  return (
    <main className="flex h-dvh flex-col overflow-hidden bg-wardian-bg text-primary" data-testid="remote-mobile-app">
      {activeRemoteTab === "watchlist" ? (
        <RemoteWatchlistView onOpenSettings={openSettings} />
      ) : activeRemoteTab === "queue" ? (
        <Suspense fallback={<RemoteSurfaceFallback />}>
          <RemoteInboxView />
        </Suspense>
      ) : (
        <RemotePlaceholderPanel tab={activeRemoteTab} />
      )}
      <RemoteBottomNav />
    </main>
  );
};

function RemotePlaceholderPanel({ tab }: { tab: "automations" | "queue" | "garden" | "library" }) {
  const label =
    tab === "automations" ? "Automations" : tab === "queue" ? "Inbox" : tab === "garden" ? "Garden" : "Library";

  return (
    <section className="flex min-h-0 flex-1 flex-col">
      <header className="shrink-0 border-b border-wardian-border bg-wardian-bg/95 px-4 py-3 backdrop-blur">
        <h1 className="truncate text-base font-semibold text-primary">{label}</h1>
      </header>
      <div className="flex flex-1 items-center justify-center px-6 text-center text-sm text-muted-neutral">
        {label} is not available in the mobile PWA yet.
      </div>
    </section>
  );
}
