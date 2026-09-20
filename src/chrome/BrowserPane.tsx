import { ArrowLeft, ChevronRight, RefreshCw } from "./icons";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  browserBounds,
  browserRequest,
  type BrowserRequest,
} from "../lib/embeddedBrowser";
import { normalizeBrowserUrl } from "../lib/browserUrl";
import { UI_SCALE_CHANGE_EVENT } from "../lib/uiScale";

export function useBrowserOpen(onShow?: () => void) {
  const [open, setOpen] = useState(false);
  const onShowRef = useRef(onShow);
  onShowRef.current = onShow;
  const show = useCallback(() => {
    onShowRef.current?.();
    setOpen(true);
  }, []);
  useEffect(() => {
    const off = getCurrentWindow().listen("open_browser", show);
    const onKey = (event: KeyboardEvent) => {
      if (
        (event.metaKey || event.ctrlKey) &&
        event.shiftKey &&
        event.key.toLowerCase() === "b"
      ) {
        event.preventDefault();
        show();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => {
      void off.then((fn) => fn());
      window.removeEventListener("keydown", onKey);
    };
  }, [show]);
  return [open, (value: boolean) => (value ? show() : setOpen(false))] as const;
}

export function BrowserPane({
  open,
  visible = true,
}: {
  open: boolean;
  visible?: boolean;
}) {
  const [ratio, setRatio] = useState(50);
  const [resizing, setResizing] = useState(false);
  const pane = useRef<HTMLDivElement>(null);
  const resize = (clientX: number) => {
    const bounds = pane.current?.parentElement?.getBoundingClientRect();
    if (bounds && bounds.width > 0)
      setRatio(
        Math.min(
          70,
          Math.max(30, ((bounds.right - clientX) / bounds.width) * 100),
        ),
      );
  };
  if (!open) return null;
  return (
    <div
      ref={pane}
      className="relative flex min-h-0 min-w-0 shrink-0 border-l border-content/10 bg-background-base"
      style={{ width: `${ratio}%` }}
    >
      <div
        role="separator"
        aria-label="Resize browser split"
        aria-orientation="vertical"
        aria-valuemin={30}
        aria-valuemax={70}
        aria-valuenow={Math.round(ratio)}
        tabIndex={0}
        className="absolute inset-y-0 -left-1 z-10 w-2 cursor-col-resize touch-none hover:bg-accent/30 focus-visible:bg-accent/30"
        onPointerDown={(event) => {
          event.preventDefault();
          event.currentTarget.setPointerCapture(event.pointerId);
          setResizing(true);
        }}
        onPointerMove={(event) => {
          if (event.currentTarget.hasPointerCapture(event.pointerId))
            resize(event.clientX);
        }}
        onPointerUp={(event) => {
          if (event.currentTarget.hasPointerCapture(event.pointerId))
            event.currentTarget.releasePointerCapture(event.pointerId);
          setResizing(false);
        }}
        onLostPointerCapture={() => setResizing(false)}
        onKeyDown={(event) => {
          if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
            event.preventDefault();
            setRatio((value) =>
              Math.min(
                70,
                Math.max(30, value + (event.key === "ArrowLeft" ? 5 : -5)),
              ),
            );
          }
        }}
      />
      <div className="flex min-h-0 min-w-0 flex-1">
        <BrowserPanel visible={visible && !resizing} />
      </div>
    </div>
  );
}

function BrowserPanel({ visible: paneVisible }: { visible: boolean }) {
  const [address, setAddress] = useState("localhost:3000");
  const [currentUrl, setCurrentUrl] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [ready, setReady] = useState(false);
  const [busy, setBusy] = useState(false);
  const host = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLInputElement>(null);
  const alive = useRef(true);
  const editing = useRef(false);
  const lastLayout = useRef("");

  const run = useCallback(async (request: BrowserRequest) => {
    try {
      const result = await browserRequest(request);
      if (alive.current) setError(null);
      return result;
    } catch (err) {
      if (alive.current) setError(String(err));
      throw err;
    }
  }, []);

  useEffect(() => {
    alive.current = true;
    setReady(false);
    setCurrentUrl(null);
    lastLayout.current = "";
    input.current?.focus();
    return () => {
      alive.current = false;
      void browserRequest({ action: "close" }).catch(() => undefined);
    };
  }, []);

  useEffect(() => {
    if (!ready) return;
    let frame = 0;
    const sync = () => {
      if (!host.current) return;
      const bounds = browserBounds(host.current);
      // Native children sit above HTML. Hide the page while app dialogs and
      // menus are displayed, otherwise it would cover their controls.
      const area = host.current.getBoundingClientRect();
      const covered = [
        ...document.querySelectorAll<HTMLElement>(
          '[role="dialog"], [role="alertdialog"], [role="menu"], [role="listbox"], [data-popover-side]',
        ),
      ].some((element) => {
        const rect = element.getBoundingClientRect();
        return (
          rect.width > 0 &&
          rect.height > 0 &&
          rect.left < area.right &&
          rect.right > area.left &&
          rect.top < area.bottom &&
          rect.bottom > area.top
        );
      });
      const visible =
        paneVisible &&
        area.width > 0 &&
        area.height > 0 &&
        !document.hidden &&
        !covered;
      const request = { action: "layout" as const, bounds, visible };
      const key = JSON.stringify(request);
      if (lastLayout.current === key) return;
      lastLayout.current = key;
      void run(request).catch(() => {
        lastLayout.current = "";
      });
    };
    const schedule = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(sync);
    };
    const resize = new ResizeObserver(schedule);
    if (host.current) resize.observe(host.current);
    const overlays = new MutationObserver(schedule);
    overlays.observe(document.body, { childList: true, subtree: true });
    window.addEventListener("resize", schedule);
    window.addEventListener(UI_SCALE_CHANGE_EVENT, schedule);
    document.addEventListener("visibilitychange", schedule);
    schedule();
    // Read the address recorded by native navigation and page-load callbacks.
    let polling = false;
    const timer = window.setInterval(() => {
      if (polling) return;
      polling = true;
      void browserRequest({ action: "url" })
        .then((url) => {
          if (!alive.current || !url) return;
          setCurrentUrl(url);
          if (!editing.current) setAddress(url);
        })
        .catch(() => undefined)
        .finally(() => {
          polling = false;
        });
    }, 700);
    return () => {
      cancelAnimationFrame(frame);
      clearInterval(timer);
      resize.disconnect();
      overlays.disconnect();
      window.removeEventListener("resize", schedule);
      window.removeEventListener(UI_SCALE_CHANGE_EVENT, schedule);
      document.removeEventListener("visibilitychange", schedule);
    };
  }, [ready, run, paneVisible]);

  const navigate = async () => {
    const url = normalizeBrowserUrl(address);
    if (!url || !host.current) {
      setError("Enter an http or https address.");
      return;
    }
    setBusy(true);
    try {
      await run({ action: "open", url, bounds: browserBounds(host.current) });
      if (alive.current) {
        setAddress(url);
        setCurrentUrl(url);
        setReady(true);
        editing.current = false;
        input.current?.blur();
      }
    } catch {
      /* run displays the native error */
    } finally {
      if (alive.current) setBusy(false);
    }
  };

  const button =
    "grid size-7 shrink-0 place-items-center rounded-md text-content/60 hover:bg-content/8 hover:text-content disabled:opacity-30 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent";
  return (
    <section
      aria-label="Browser"
      className="flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden bg-background-base text-content"
    >
      <form
        className="flex h-10 shrink-0 items-center gap-0.5 border-b border-content/10 px-2"
        onSubmit={(event) => {
          event.preventDefault();
          void navigate();
        }}
      >
        {(
          [
            ["back", "Back", ArrowLeft],
            ["forward", "Forward", ChevronRight],
            ["reload", "Reload", RefreshCw],
          ] as const
        ).map(([action, title, Icon]) => (
          <button
            key={action}
            aria-label={title}
            title={title}
            type="button"
            className={button}
            disabled={!ready}
            onClick={() => void run({ action }).catch(() => undefined)}
          >
            <Icon className="size-3.5" strokeWidth={1.75} />
          </button>
        ))}
        <input
          ref={input}
          aria-label="Browser address"
          value={address}
          spellCheck={false}
          autoCapitalize="off"
          autoCorrect="off"
          onFocus={() => {
            editing.current = true;
          }}
          onChange={(event) => setAddress(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Escape") {
              editing.current = false;
              setAddress(currentUrl ?? address);
              input.current?.blur();
            }
          }}
          className="mx-1 h-7 min-w-0 flex-1 rounded-md border border-content/15 bg-content/5 px-2 text-xs outline-none focus:ring-2 focus:ring-accent"
        />
        <button
          type="submit"
          className={button}
          disabled={busy}
          aria-label="Go to address"
          title="Go to address"
        >
          <ChevronRight className="size-3.5" strokeWidth={1.75} />
        </button>
      </form>
      {error && (
        <p role="alert" className="shrink-0 px-3 py-2 text-xs text-red-400">
          {error}
        </p>
      )}
      <div ref={host} className="min-h-0 flex-1 bg-white">
        {!ready && (
          <p className="p-5 text-sm text-neutral-600">
            Enter a website or local development server address.
          </p>
        )}
      </div>
    </section>
  );
}
