import React, { useLayoutEffect } from "react";
import ReactDOM from "react-dom/client";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import App from "./app/App";
import { activateWindowAppearance, initAppearance } from "./features/settings/model/appearance";
import { initSounds } from "./features/settings/model/sounds";
import {
  abortQuit,
  askQuitConfirmation,
  commitQuit,
  loadBootWorkspace,
  reportQuitPoll,
} from "./app/model/appLifecycle";
import { homeDir } from "./platform/tauri/fs";
import { setHomeDir } from "./shared/lib/paths";
import { consumeInstalledUpdate } from "./app/model/updateNotice";
import "./styles/index.css";

initAppearance();
initSounds();
// Prime the real home directory before the first render so every `~/` file
// reference resolves consistently. The IPC call is local and failures remain
// best-effort, falling back to inference from a session's cwd.
const homeDirPrimed = homeDir()
  .then(setHomeDir)
  .catch(() => {});

function dismissBootSplash() {
  const splash = document.getElementById("boot-splash");
  if (!splash || splash.dataset.dismissed === "1") return;
  splash.dataset.dismissed = "1";
  const fade = () => {
    activateWindowAppearance();
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

void listen<number>("quit_poll", (event) => {
  void reportQuitPoll(event.payload);
});
// Scoped to this window on purpose: a global `listen` is registered as `Any`,
// which Tauri matches for every event regardless of the emitter's target, so
// one dialog would become one per window.
void getCurrentWebviewWindow().listen<{ id: number; inFlight: number }>(
  "quit_confirm",
  (event) => {
    void askQuitConfirmation(event.payload.id, event.payload.inFlight);
  },
);
void listen<number>("quit_commit", (event) => {
  void commitQuit(event.payload);
});
void listen("quit_aborted", () => {
  abortQuit();
});

void Promise.all([homeDirPrimed, loadBootWorkspace()]).then(
  ([, { windowTransfer, resumed, history, historyCwd }]) => {
    const installedUpdate = windowTransfer ? null : consumeInstalledUpdate();
    ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
      <React.StrictMode>
        <BootGate>
          <App
            windowTransfer={windowTransfer}
            resumed={resumed}
            installedUpdate={installedUpdate}
            history={history}
            historyCwd={historyCwd}
          />
        </BootGate>
      </React.StrictMode>,
    );
  },
);
