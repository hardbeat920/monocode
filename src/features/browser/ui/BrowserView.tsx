import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  useSyncExternalStore,
  type FormEvent,
} from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  ChevronLeft,
  ChevronRight,
  ExternalLink,
  RefreshCw,
  X,
} from "../../../shared/ui/icons";
import { IconButton } from "../../../app/shell/TitleBar";
import {
  browserHistory,
  closeBrowserView,
  navigateBrowser,
  openBrowserView,
  setBrowserBounds,
  setBrowserVisible,
  type BrowserBounds,
} from "../../../platform/tauri/browser";
import {
  loadUiScale,
  subscribeUiScale,
} from "../../settings/model/uiScale";
import type { BrowserTabSource } from "../../workspace/model/layout";
import { BLANK_URL, browserUrlFromInput } from "../model/browserUrl";
import { patchBrowserTab } from "../model/browserStore";
import { useOverlayOcclusion } from "../hooks/useOverlayOcclusion";

type Props = {
  id: string;
  tab: BrowserTabSource;
  /** Active tab of a dock that is on screen. */
  visible: boolean;
  /** The dock sash is being dragged. */
  resizing: boolean;
};

/**
 * React StrictMode and dock moves unmount and remount a view in one turn.
 * Defer the native close so a remount can take the live page back.
 */
const pendingClose = new Map<string, ReturnType<typeof setTimeout>>();

function sameBounds(a: BrowserBounds | null, b: BrowserBounds): boolean {
  return (
    !!a &&
    a.x === b.x &&
    a.y === b.y &&
    a.width === b.width &&
    a.height === b.height
  );
}

export function BrowserView({ id, tab, visible, resizing }: Props) {
  const host = useRef<HTMLDivElement>(null);
  const scale = useSyncExternalStore(subscribeUiScale, loadUiScale);
  const covered = useOverlayOcclusion(host, visible);
  const shown = visible && !covered;
  const [ready, setReady] = useState(false);
  const [address, setAddress] = useState(tab.url === BLANK_URL ? "" : tab.url);
  const [editing, setEditing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const placed = useRef<BrowserBounds | null>(null);
  const input = useRef<HTMLInputElement>(null);

  const measure = useCallback((): BrowserBounds | null => {
    const el = host.current;
    if (!el) return null;
    const rect = el.getBoundingClientRect();
    return {
      x: Math.round(rect.left * scale),
      y: Math.round(rect.top * scale),
      width: Math.max(1, Math.round(rect.width * scale)),
      height: Math.max(1, Math.round(rect.height * scale)),
    };
  }, [scale]);

  // Create the native page once; a quick remount reuses it.
  useLayoutEffect(() => {
    const queued = pendingClose.get(id);
    if (queued) {
      clearTimeout(queued);
      pendingClose.delete(id);
    }
    const bounds = measure() ?? { x: 0, y: 0, width: 1, height: 1 };
    placed.current = bounds;
    let alive = true;
    openBrowserView(id, tab.url, bounds, false)
      .then(() => alive && setReady(true))
      .catch((reason: unknown) => alive && setError(String(reason)));
    return () => {
      alive = false;
      setReady(false);
      pendingClose.set(
        id,
        setTimeout(() => {
          pendingClose.delete(id);
          void closeBrowserView(id).catch(() => undefined);
        }, 0),
      );
    };
    // Navigation after mount goes through navigateBrowser, not a remount.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);

  useEffect(() => {
    if (!ready) return;
    void setBrowserVisible(id, shown).catch(() => undefined);
  }, [id, ready, shown]);

  // Follow the placeholder. Size changes come from the observer; position-only
  // moves (a sidebar opening beside a left dock) from the window and layout.
  useEffect(() => {
    if (!ready || !visible) return;
    const el = host.current;
    if (!el) return;
    let frame: number | null = null;
    const sync = () => {
      frame = null;
      const bounds = measure();
      if (!bounds || sameBounds(placed.current, bounds)) return;
      placed.current = bounds;
      void setBrowserBounds(id, bounds).catch(() => undefined);
    };
    const schedule = () => {
      if (frame == null) frame = requestAnimationFrame(sync);
    };
    const observer = new ResizeObserver(schedule);
    observer.observe(el);
    const main = el.closest("main");
    if (main) observer.observe(main);
    window.addEventListener("resize", schedule);
    schedule();
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", schedule);
      if (frame != null) cancelAnimationFrame(frame);
    };
  }, [id, measure, ready, visible]);

  // While the sash is dragged the dock repaints every frame.
  useEffect(() => {
    if (!resizing || !ready) return;
    let frame = requestAnimationFrame(function follow() {
      const bounds = measure();
      if (bounds && !sameBounds(placed.current, bounds)) {
        placed.current = bounds;
        void setBrowserBounds(id, bounds).catch(() => undefined);
      }
      frame = requestAnimationFrame(follow);
    });
    return () => cancelAnimationFrame(frame);
  }, [id, measure, ready, resizing]);

  useEffect(() => {
    if (!editing) setAddress(tab.url === BLANK_URL ? "" : tab.url);
  }, [editing, tab.url]);

  useEffect(() => {
    if (ready && visible && tab.url === BLANK_URL) input.current?.focus();
    // Only when a blank tab first appears.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ready, visible]);

  const go = (event: FormEvent) => {
    event.preventDefault();
    const url = browserUrlFromInput(address);
    setEditing(false);
    setError(null);
    input.current?.blur();
    patchBrowserTab(id, { url, loading: true });
    void navigateBrowser(id, url).catch((reason: unknown) => {
      patchBrowserTab(id, { loading: false });
      setError(String(reason));
    });
  };

  return (
    <div className="flex h-full min-h-0 flex-col">
      <form
        className="flex h-8 shrink-0 items-center gap-0.5 border-b border-stroke px-1.5"
        onSubmit={go}
      >
        <IconButton
          label="Back"
          onClick={() => void browserHistory(id, "back").catch(() => undefined)}
        >
          <ChevronLeft className="size-3.5" strokeWidth={1.75} />
        </IconButton>
        <IconButton
          label="Forward"
          onClick={() =>
            void browserHistory(id, "forward").catch(() => undefined)
          }
        >
          <ChevronRight className="size-3.5" strokeWidth={1.75} />
        </IconButton>
        <IconButton
          label={tab.loading ? "Stop" : "Reload"}
          onClick={() =>
            void browserHistory(id, tab.loading ? "stop" : "reload").catch(
              () => undefined,
            )
          }
        >
          {tab.loading ? (
            <X className="size-3.5" strokeWidth={1.75} />
          ) : (
            <RefreshCw className="size-3.5" strokeWidth={1.75} />
          )}
        </IconButton>
        <input
          ref={input}
          aria-label="Address"
          spellCheck={false}
          autoCapitalize="off"
          autoCorrect="off"
          placeholder="Search or enter address"
          className="mx-1 h-6 min-w-0 flex-1 rounded-md bg-content/5 px-2 text-[12px] text-content outline-none placeholder:text-content/35 focus:bg-content/10"
          value={address}
          onFocus={(event) => {
            setEditing(true);
            event.currentTarget.select();
          }}
          onBlur={() => setEditing(false)}
          onChange={(event) => setAddress(event.target.value)}
          onKeyDown={(event) => {
            if (event.key !== "Escape") return;
            setEditing(false);
            setAddress(tab.url === BLANK_URL ? "" : tab.url);
            event.currentTarget.blur();
          }}
        />
        <IconButton
          label="Open in Default Browser"
          onClick={() => {
            if (tab.url !== BLANK_URL) void openUrl(tab.url).catch(() => undefined);
          }}
        >
          <ExternalLink className="size-3.5" strokeWidth={1.75} />
        </IconButton>
      </form>
      <div ref={host} className="relative min-h-0 flex-1 bg-background-base">
        {error ? (
          <div className="absolute inset-0 grid place-items-center p-6 text-center text-[12px] text-content/55">
            {error}
          </div>
        ) : null}
      </div>
    </div>
  );
}
