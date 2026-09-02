import "./lib/cryptoPolyfill";
import React, { useLayoutEffect } from "react";
import ReactDOM from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { RemoteGate } from "./bridge/RemoteGate";
import { isRemoteSession } from "./bridge/remoteClient";
import App from "./App";
import { initAppearance } from "./lib/appearance";
import { initRemoteAccess } from "./lib/remote";
import { initSounds } from "./lib/sounds";
import { handleQuitRequested, loadBootWorkspace } from "./lib/appLifecycle";
import { consumeInstalledUpdate } from "./lib/updateNotice";
import { isTauriRuntime } from "./bridge/isTauri";
import "./index.css";

initAppearance();
initSounds();
if (isTauriRuntime()) {
  void initRemoteAccess();
}

function dismissBootSplash() {
  const splash = document.getElementById("boot-splash");
  if (!splash || splash.dataset.dismissed === "1") return;
  splash.dataset.dismissed = "1";
  const fade = () => {
    if (!isRemoteSession()) {
      void invoke("enable_window_glass");
    }
    splash.classList.add("boot-splash-out");
    window.setTimeout(() => splash.remove(), 180);
  };
  // useLayoutEffect runs before paint. Two frames later the app is on
  // screen, so the fade reveals UI instead of the desktop blur.
  requestAnimationFrame(() => {
    requestAnimationFrame(fade);
  });
}

function BootGate({ children }: { children: React.ReactNode }) {
  useLayoutEffect(() => {
    dismissBootSplash();
  }, []);
  return children;
}

void listen("quit_requested", () => {
  void handleQuitRequested();
});

void loadBootWorkspace().then(
  ({ windowTransfer, resumed, history, historyCwd }) => {
    const installedUpdate = windowTransfer ? null : consumeInstalledUpdate();
    ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
      <React.StrictMode>
        <RemoteGate>
          <BootGate>
            <App
              windowTransfer={windowTransfer}
              resumed={resumed}
              installedUpdate={installedUpdate}
              history={history}
              historyCwd={historyCwd}
            />
          </BootGate>
        </RemoteGate>
      </React.StrictMode>,
    );
  },
);
