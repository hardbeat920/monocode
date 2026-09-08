import React, { useCallback, useEffect, useLayoutEffect, useState } from "react";
import ReactDOM from "react-dom/client";
// TRANSPORT SEAM: see src/lib/transport/.
import { invoke, listen } from "./lib/transport";
import App from "./App";
import { initAppearance } from "./lib/appearance";
import { initSounds } from "./lib/sounds";
import { IS_IPAD } from "./lib/platform";
import {
  getCompanionStatus,
  loadCompanionMode,
  onCompanionStatusChange,
  waitForCompanionLink,
} from "./lib/transport";
import {
  handleQuitRequested,
  loadBootWorkspace,
  type BootWorkspace,
} from "./lib/appLifecycle";
import { CompanionPairing } from "./surfaces/CompanionPairing";
import {
  consumeInstalledUpdate,
  type InstalledUpdate,
} from "./lib/updateNotice";
import "./index.css";

initAppearance();
initSounds();

function dismissBootSplash() {
  const splash = document.getElementById("boot-splash");
  if (!splash || splash.dataset.dismissed === "1") return;
  splash.dataset.dismissed = "1";
  const fade = () => {
    void invoke("enable_window_glass");
    splash.classList.add("boot-splash-out");
    window.setTimeout(() => splash.remove(), 180);
  };
  requestAnimationFrame(() => {
    requestAnimationFrame(fade);
  });
}

void listen("quit_requested", () => {
  void handleQuitRequested();
});

const COMPANION_FIRST = loadCompanionMode() || IS_IPAD;

function BootRoot() {
  const [paired, setPaired] = useState(
    () => !COMPANION_FIRST || getCompanionStatus() === "connected",
  );
  const [workspace, setWorkspace] = useState<BootWorkspace | null>(null);
  const [installedUpdate, setInstalledUpdate] =
    useState<InstalledUpdate | null>(null);

  const enterApp = useCallback(() => {
    setPaired(true);
  }, []);

  useLayoutEffect(() => {
    dismissBootSplash();
  }, []);

  useEffect(() => {
    if (!COMPANION_FIRST) return;
    const stop = onCompanionStatusChange((status) => {
      if (status === "connected") enterApp();
    });
    void waitForCompanionLink().then(() => {
      if (getCompanionStatus() === "connected") enterApp();
    });
    return stop;
  }, [enterApp]);

  useEffect(() => {
    if (COMPANION_FIRST && !paired) return;
    let cancelled = false;
    void loadBootWorkspace().then((next) => {
      if (cancelled) return;
      setWorkspace(next);
      setInstalledUpdate(
        next.windowTransfer ? null : consumeInstalledUpdate(),
      );
    });
    return () => {
      cancelled = true;
    };
  }, [paired]);

  if (COMPANION_FIRST && !paired) {
    return <CompanionPairing onConnected={enterApp} />;
  }
  if (!workspace) {
    return COMPANION_FIRST ? <CompanionPairing onConnected={enterApp} /> : null;
  }
  return (
    <App
      windowTransfer={workspace.windowTransfer}
      resumed={workspace.resumed}
      installedUpdate={installedUpdate}
      history={workspace.history}
      historyCwd={workspace.historyCwd}
    />
  );
}

const root = document.getElementById("root");
if (!root) {
  throw new Error("missing #root");
}
ReactDOM.createRoot(root).render(
  <React.StrictMode>
    <BootRoot />
  </React.StrictMode>,
);
