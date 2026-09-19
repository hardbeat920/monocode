import React, { useLayoutEffect } from "react";
import ReactDOM from "react-dom/client";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import App from "./App";
import { activateWindowAppearance, initAppearance } from "./lib/appearance";
import { initSounds } from "./lib/sounds";
import {
  abortQuit,
  askQuitConfirmation,
  commitQuit,
  loadBootWorkspace,
  reportQuitPoll,
} from "./lib/appLifecycle";
import { consumeInstalledUpdate } from "./lib/updateNotice";
import "./index.css";

initAppearance();
initSounds();
if (import.meta.env.DEV) document.title = "MonoCode — boot start";

function showBootError(message: string) {
  const host = document.getElementById("boot-splash") ?? document.body;
  const note = document.createElement("pre");
  note.style.cssText =
    "position:fixed;inset:auto 12px 12px 12px;z-index:9999;max-height:40vh;overflow:auto;" +
    "padding:8px 10px;border-radius:8px;background:rgba(127,29,29,.92);color:#fecaca;" +
    "font:11px/1.5 ui-monospace,monospace;white-space:pre-wrap;";
  note.textContent = `MonoCode failed to start:\n${message}`;
  host.append(note);
}

// The webview injects __TAURI_INTERNALS__ per navigation; after an HMR full
// reload, module evaluation can race ahead of the injection. Every top-level
// Tauri API call must therefore wait for readiness instead of crashing the
// module before any error reporting exists — that failure mode leaves the
// boot splash up forever with nothing in the console.
function whenTauriReady(run: () => void) {
  // Injection is two-phase: the internals object appears first, then invoke
  // and friends get attached. Wait for a callable invoke.
  const ready = () =>
    typeof (window as unknown as {
      __TAURI_INTERNALS__?: { invoke?: unknown };
    }).__TAURI_INTERNALS__?.invoke === "function";
  if (ready()) {
    run();
    return;
  }
  let tries = 0;
  const timer = window.setInterval(() => {
    tries += 1;
    if (ready() || tries > 50) {
      window.clearInterval(timer);
      if (ready()) {
        run();
        return;
      }
      const internals = (window as unknown as Record<string, unknown>)[
        "__TAURI_INTERNALS__"
      ] as Record<string, unknown> | undefined;
      const ipc = (window as unknown as Record<string, unknown>)["ipc"];
      showBootError(
        [
          "Tauri IPC never became available on this page.",
          `origin: ${window.location.origin}`,
          `readyState: ${document.readyState}`,
          `internals: ${internals ? Object.keys(internals).join(",") || "empty" : "absent"}`,
          `window.ipc: ${typeof ipc}`,
        ].join("\n"),
      );
    }
  }, 100);
}

function dismissBootSplash() {
  const splash = document.getElementById("boot-splash");
  if (!splash || splash.dataset.dismissed === "1") return;
  splash.dataset.dismissed = "1";
  let revealed = false;
  const fade = () => {
    if (revealed) return;
    revealed = true;
    activateWindowAppearance();
    splash.classList.add("boot-splash-out");
    window.setTimeout(() => splash.remove(), 180);
  };
  // useLayoutEffect runs before paint. Two frames later the app is on
  // screen, so the fade reveals UI instead of the desktop blur. A dev
  // webview whose compositor never fires rAF (background-launched windows)
  // would keep the splash forever — the timer forces the reveal.
  requestAnimationFrame(() => {
    requestAnimationFrame(fade);
  });
  window.setTimeout(fade, 1200);
}

function BootGate({ children }: { children: React.ReactNode }) {
  useLayoutEffect(() => {
    dismissBootSplash();
  }, []);
  return children;
}

whenTauriReady(() => {
  // `listen` registers asynchronously; a rejected registration never reaches
  // a synchronous `catch`, so each promise carries its own.
  const wired = (promise: Promise<unknown>) =>
    promise.catch((error: unknown) => {
      showBootError(
        `quit wiring failed: ${error instanceof Error ? error.message : String(error)}`,
      );
    });
  wired(listen<number>("quit_poll", (event) => {
    void reportQuitPoll(event.payload);
  }));
  // Scoped to this window on purpose: a global `listen` is registered as
  // `Any`, which Tauri matches for every event regardless of the
  // emitter's target, so one dialog would become one per window.
  wired(getCurrentWebviewWindow().listen<{ id: number; inFlight: number }>(
    "quit_confirm",
    (event) => {
      void askQuitConfirmation(event.payload.id, event.payload.inFlight);
    },
  ));
  wired(listen<number>("quit_commit", (event) => {
    void commitQuit(event.payload);
  }));
  wired(listen("quit_aborted", () => {
    abortQuit();
  }));
});

whenTauriReady(() => {
  void loadBootWorkspace().then(
    ({ windowTransfer, resumed, history, historyCwd }) => {
      document.title = "MonoCode";
      const installedUpdate = windowTransfer ? null : consumeInstalledUpdate();
      ReactDOM.createRoot(
        document.getElementById("root") as HTMLElement,
      ).render(
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
    (error: unknown) => {
      // A rejected boot promise would otherwise leave the splash up forever.
      showBootError(error instanceof Error ? error.message : String(error));
    },
  );
});

if (import.meta.env.DEV) {
  // First-paint crashes leave the splash up with nothing in the console of a
  // packaged build; mirror them onto the splash during development.
  window.addEventListener("error", (event) => {
    if (document.querySelector("#boot-splash [data-dismissed]")) return;
    if (document.getElementById("root")?.childElementCount) return;
    showBootError(
      `${event.message}\n  at ${event.filename ?? "?"}:${event.lineno ?? "?"}`,
    );
  });
  window.addEventListener("unhandledrejection", (event) => {
    if (document.getElementById("root")?.childElementCount) return;
    showBootError(String(event.reason));
  });
}
