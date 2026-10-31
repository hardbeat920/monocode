import {
  type FormEvent,
  type KeyboardEvent,
  type MouseEvent,
  type WheelEvent,
  useCallback,
  useEffect,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import {
  ChevronLeft,
  ChevronRight,
  Globe,
  Lock,
  Plus,
  RefreshCw,
} from "../../../shared/ui/icons";
import {
  type BrowserLiveFrame,
  type BrowserLiveInput,
  type BrowserLiveNav,
  type BrowserTab,
  cancelBrowserPick,
  navigateBrowserLive,
  newBrowserTab,
  pullBrowserLiveFrame,
  resizeBrowserLive,
  sendBrowserLiveInput,
  startBrowserLive,
  stopBrowserLive,
  useBrowserState,
} from "../../settings/model/browser";
import { TabLabel } from "../../../shared/ui/TabLabel";
import {
  browserPickTab,
  markBrowserTabShown,
  requestBrowserPane,
  subscribeBrowserPick,
} from "../model/browserPane";
import { type Drawn, LivePainter, type PaintFrame } from "./livePainter";
import type { PainterMessage } from "./livePainter.worker";

/** CDP modifier bits: Alt 1, Ctrl 2, Meta 4, Shift 8. */
export function liveModifiers(event: {
  altKey: boolean;
  ctrlKey: boolean;
  metaKey: boolean;
  shiftKey: boolean;
}): number {
  return (
    (event.altKey ? 1 : 0) |
    (event.ctrlKey ? 2 : 0) |
    (event.metaKey ? 4 : 0) |
    (event.shiftKey ? 8 : 0)
  );
}

/** Keys that edit or move rather than type, with their virtual key codes. */
const RAW_KEYS: Record<string, number> = {
  Backspace: 8,
  Tab: 9,
  Enter: 13,
  Escape: 27,
  PageUp: 33,
  PageDown: 34,
  End: 35,
  Home: 36,
  ArrowLeft: 37,
  ArrowUp: 38,
  ArrowRight: 39,
  ArrowDown: 40,
  Delete: 46,
};

/**
 * The CDP input for a key the user pressed in the live view, for editing
 * keys and shortcuts. `null` for everything that types: text reaches the
 * page from the capture field's input events, so keyboard layouts, dead
 * keys, IME and paste all arrive as the text they produced.
 */
export function liveKeyInput(
  event: Pick<
    globalThis.KeyboardEvent,
    | "key"
    | "code"
    | "altKey"
    | "ctrlKey"
    | "metaKey"
    | "shiftKey"
    | "isComposing"
  >,
  phase: "down" | "up",
): BrowserLiveInput | null {
  if (event.isComposing) return null;
  const modifiers = liveModifiers(event);
  const base = { kind: "key" as const, key: event.key, code: event.code };
  if (event.key in RAW_KEYS) {
    const keyCode = RAW_KEYS[event.key];
    if (phase === "up") return { ...base, event: "keyUp", keyCode, modifiers };
    return event.key === "Enter"
      ? { ...base, event: "keyDown", text: "\r", keyCode, modifiers }
      : { ...base, event: "rawKeyDown", keyCode, modifiers };
  }
  // Ctrl/Cmd + a letter or digit: a shortcut for the page (select all, undo).
  // Ctrl+V stays with the capture field, which pastes it as text.
  const shortcut =
    (event.ctrlKey || event.metaKey) &&
    !event.altKey &&
    /^[a-z0-9]$/i.test(event.key) &&
    event.key.toLowerCase() !== "v";
  if (!shortcut) return null;
  const keyCode = event.key.toUpperCase().charCodeAt(0);
  return {
    ...base,
    event: phase === "up" ? "keyUp" : "rawKeyDown",
    keyCode,
    modifiers,
  };
}

/** A point on the shown picture, in the page's CSS pixels. */
export function livePoint(
  client: { x: number; y: number },
  picture: { left: number; top: number; width: number; height: number },
  frame: { width: number; height: number },
): { x: number; y: number } {
  const x = ((client.x - picture.left) / picture.width) * frame.width;
  const y = ((client.y - picture.top) / picture.height) * frame.height;
  return {
    x: Math.round(Math.min(Math.max(x, 0), frame.width) * 10) / 10,
    y: Math.round(Math.min(Math.max(y, 0), frame.height) * 10) / 10,
  };
}

/**
 * Add `input` to the inputs still waiting to go to the tab. Moves and
 * scrolls that follow one another become one: the pointer's latest place,
 * and the scroll distances added up, so a fast wheel or trackpad never
 * builds a backlog the page works through long after the hand has stopped.
 */
export function queueLiveInput(
  queue: BrowserLiveInput[],
  input: BrowserLiveInput,
): void {
  const last = queue[queue.length - 1];
  if (
    last?.kind === "wheel" &&
    input.kind === "wheel" &&
    (last.modifiers ?? 0) === (input.modifiers ?? 0)
  ) {
    queue[queue.length - 1] = {
      ...input,
      deltaX: last.deltaX + input.deltaX,
      deltaY: last.deltaY + input.deltaY,
    };
    return;
  }
  if (
    last?.kind === "mouse" &&
    last.event === "mouseMoved" &&
    input.kind === "mouse" &&
    input.event === "mouseMoved" &&
    last.buttons === input.buttons &&
    last.modifiers === input.modifiers
  ) {
    queue[queue.length - 1] = input;
    return;
  }
  queue.push(input);
}

/** Ctrl+T (Cmd+T on a Mac): a new tab, as in a browser. */
export function isNewTabKey(
  event: Pick<
    globalThis.KeyboardEvent,
    "key" | "altKey" | "ctrlKey" | "metaKey" | "shiftKey"
  >,
): boolean {
  return (
    (event.ctrlKey || event.metaKey) &&
    !event.altKey &&
    !event.shiftKey &&
    event.key.toLowerCase() === "t"
  );
}

/** A wheel's distance in CSS pixels, whatever unit the event counts in. */
export function wheelPixels(delta: number, mode: number, page: number): number {
  if (mode === 1) return delta * 40;
  if (mode === 2) return delta * page;
  return delta;
}

/**
 * Where to put the picture so its pixels land on the screen's: a pane that
 * starts between two device pixels would have every picture resampled,
 * which softens text. The CSS offset that moves `edge` onto the next one.
 */
export function pixelSnap(edge: number, scale: number): number {
  const device = edge * scale;
  return (Math.round(device) - device) / scale;
}

/** The address split the way the address field shows it: host strong, the rest quiet. */
export function liveAddress(url: string): {
  secure: boolean;
  host: string;
  rest: string;
} {
  try {
    const parsed = new URL(url);
    if (parsed.protocol !== "http:" && parsed.protocol !== "https:") {
      return { secure: false, host: url, rest: "" };
    }
    const rest = `${parsed.pathname === "/" ? "" : parsed.pathname}${parsed.search}${parsed.hash}`;
    return { secure: parsed.protocol === "https:", host: parsed.host, rest };
  } catch {
    return { secure: false, host: url, rest: "" };
  }
}

const BUTTONS = ["left", "middle", "right"] as const;

/** The live view's painter, wherever it runs. */
type Painter = {
  frame(frame: PaintFrame): void;
  close(): void;
};

/**
 * A painter for `canvas`: in a worker that draws on it as an
 * OffscreenCanvas, so a busy main thread (a transcript streaming) never
 * holds a picture back. A webview without OffscreenCanvas draws here.
 */
function livePainter(
  canvas: HTMLCanvasElement,
  drawn: (shown: Drawn) => void,
): Painter {
  if (
    typeof Worker === "function" &&
    typeof canvas.transferControlToOffscreen === "function"
  ) {
    let worker: Worker | undefined;
    try {
      worker = new Worker(new URL("./livePainter.worker.ts", import.meta.url), {
        type: "module",
      });
      const offscreen = canvas.transferControlToOffscreen();
      const painter = worker;
      const post = (message: PainterMessage, transfer: Transferable[] = []) =>
        painter.postMessage(message, transfer);
      post({ kind: "canvas", canvas: offscreen }, [offscreen]);
      painter.onmessage = ({ data }: MessageEvent<Drawn>) => drawn(data);
      return {
        frame: (frame) =>
          post({ kind: "frame", ...frame }, [frame.picture.buffer]),
        close: () => post({ kind: "close" }),
      };
    } catch {
      // No worker, or the canvas could not be handed over: it is drawn here.
      worker?.terminate();
    }
  }
  return new LivePainter(canvas, drawn);
}

/** A ring where the user pressed, gone in a moment. */
function pressRipple(stage: HTMLElement, x: number, y: number) {
  const rect = stage.getBoundingClientRect();
  const ring = document.createElement("span");
  ring.setAttribute("aria-hidden", "true");
  ring.className =
    "pointer-events-none absolute size-7 rounded-full border-2 border-teal-500/70 bg-teal-400/15";
  ring.style.left = `${x - rect.left - 14}px`;
  ring.style.top = `${y - rect.top - 14}px`;
  stage.appendChild(ring);
  const done = () => ring.remove();
  if (typeof ring.animate !== "function") {
    window.setTimeout(done, 300);
    return;
  }
  ring
    .animate(
      [
        { transform: "scale(0.45)", opacity: 1 },
        { transform: "scale(1)", opacity: 0 },
      ],
      { duration: 320, easing: "cubic-bezier(0.2, 0.7, 0.3, 1)" },
    )
    .finished.then(done, done);
}

/** The thread's tab as the browser module knows it now, while it is the thread's. */
export function useLiveBrowserTab(
  sessionId: string,
  tab: string,
): BrowserTab | undefined {
  const state = useBrowserState();
  return state?.tabs.find(
    (entry) => entry.tab === tab && entry.session_id === sessionId,
  );
}

/** A browser pane's tab label: the page's title as it changes. */
export function BrowserPaneTitle({
  sessionId,
  tab,
  fallback,
  className,
}: {
  sessionId: string;
  tab: string;
  fallback: string;
  className?: string;
}) {
  const live = useLiveBrowserTab(sessionId, tab);
  return <TabLabel className={className}>{live?.title || fallback}</TabLabel>;
}

type Phase =
  | { kind: "connecting" }
  | { kind: "live" }
  | { kind: "elsewhere" }
  | { kind: "gone" }
  | { kind: "failed"; message: string };

/**
 * "Show in MonoCode": one of the thread's browser tabs, live in a workspace
 * pane. The picture comes from the browser, which keeps painting the tab in
 * the background at the pane's size; clicks, scrolling and typing go back to
 * the tab. Only one pane streams at a time, so a pane that is hidden stops
 * and the one shown last takes over.
 *
 * The pane asks for pictures one after another: each request is answered
 * with the newest picture once there is one, the next request goes out the
 * moment a picture arrives, and decoding and drawing happen meanwhile. A
 * pane that cannot keep up gets the picture of now, never a backlog.
 */
export function BrowserLiveView({
  sessionId,
  tab: tabId,
  visible,
  focused = visible,
}: {
  sessionId: string;
  tab: string;
  visible: boolean;
  focused?: boolean;
}) {
  const tab = useLiveBrowserTab(sessionId, tabId);
  const stage = useRef<HTMLDivElement>(null);
  /** Holds the canvas the pictures are drawn on, at the page's CSS size. */
  const surface = useRef<HTMLDivElement>(null);
  const capture = useRef<HTMLTextAreaElement>(null);
  /** The picture on screen: its page size, for input coordinates. */
  const frame = useRef<Pick<BrowserLiveFrame, "width" | "height"> | null>(null);
  const [shown, setShown] = useState(false);
  const [fps, setFps] = useState(0);
  const [phase, setPhase] = useState<Phase>({ kind: "connecting" });
  const [error, setError] = useState<string | null>(null);
  const [attempt, setAttempt] = useState(0);
  const open = tab?.state === "open";
  const picking =
    useSyncExternalStore(subscribeBrowserPick, browserPickTab, () => null) ===
    tabId;

  useEffect(() => {
    if (!visible || !open) return;
    return markBrowserTabShown(tabId);
  }, [tabId, visible, open]);

  useEffect(() => {
    if (!visible || !open) return;
    const box = stage.current;
    const host = surface.current;
    if (!box || !host) return;
    let live = true;
    // This pane's own view: stopping it never stops a newer pane's.
    const view = crypto.randomUUID();
    let count = 0;
    let started = false;
    let ended = false;
    let resizeTimer = 0;
    let sized = "";
    // A canvas of this view's own: one handed to a worker stays the worker's.
    const canvas = document.createElement("canvas");
    canvas.className = "block size-full";
    host.replaceChildren(canvas);
    const meter = window.setInterval(() => {
      setFps(count);
      count = 0;
    }, 1000);
    // The tab's viewport takes the pane's size, so the page fills it.
    const size = () => {
      const rect = box.getBoundingClientRect();
      return {
        width: Math.max(1, Math.round(rect.width)),
        height: Math.max(1, Math.round(rect.height)),
        scale: window.devicePixelRatio || 1,
      };
    };
    const observer = new ResizeObserver(() => {
      if (!started) return;
      window.clearTimeout(resizeTimer);
      resizeTimer = window.setTimeout(() => {
        const { width, height, scale } = size();
        void resizeBrowserLive(view, width, height, scale).catch(() => {});
      }, 120);
    });
    observer.observe(box);
    // A picture is on screen: the surface takes the page's CSS size (one
    // picture pixel per device pixel, never stretched to the pane, which
    // blurs text), on whole device pixels.
    const drawn = (shown: Drawn) => {
      if (!live) return;
      count += 1;
      const scale = window.devicePixelRatio || 1;
      const rect = box.getBoundingClientRect();
      const place = `${shown.width}x${shown.height}@${rect.left},${rect.top},${scale}`;
      if (place !== sized) {
        sized = place;
        host.style.width = `${shown.width}px`;
        host.style.height = `${shown.height}px`;
        host.style.transform = `translate(${pixelSnap(rect.left, scale)}px, ${pixelSnap(rect.top, scale)}px)`;
      }
      frame.current = { width: shown.width, height: shown.height };
      setShown(true);
      setPhase((current) =>
        current.kind === "live" ? current : { kind: "live" },
      );
    };
    const painter = livePainter(canvas, drawn);
    // Ask for the next picture as soon as the last one is in hand, so a
    // request always waits at the browser module while this one is decoded
    // and drawn. An empty answer (nothing new for a second) is asked again;
    // once the view has ended, the asking stops.
    const pull = async () => {
      let after = 0;
      let empty = 0;
      while (live && !ended) {
        let next: BrowserLiveFrame | null = null;
        try {
          next = await pullBrowserLiveFrame(view, after);
        } catch {
          if (!live || ended) return;
          await new Promise((r) => setTimeout(r, 250));
          continue;
        }
        if (!next) {
          // A view that is gone answers empty at once, until the start
          // call settles; a few quick empties in a row are not a loop.
          const now = performance.now();
          if (now - empty < 50) await new Promise((r) => setTimeout(r, 100));
          empty = now;
          continue;
        }
        after = Math.max(after, next.seq);
        if (live) painter.frame(next);
      }
    };
    setPhase({ kind: "connecting" });
    {
      const { width, height, scale } = size();
      started = true;
      startBrowserLive(sessionId, tabId, view, width, height, scale)
        .then((end) => {
          ended = true;
          if (!live) return;
          // Stopped while this pane still shows: another pane took over.
          setPhase({ kind: end === "tab_gone" ? "gone" : "elsewhere" });
        })
        .catch((err: unknown) => {
          ended = true;
          if (live)
            setPhase({
              kind: "failed",
              message: err instanceof Error ? err.message : String(err),
            });
        });
      void pull();
    }
    return () => {
      live = false;
      observer.disconnect();
      window.clearTimeout(resizeTimer);
      window.clearInterval(meter);
      painter.close();
      void stopBrowserLive(view).catch(() => {});
    };
  }, [sessionId, tabId, visible, open, attempt]);

  // A blank tab waits for an address; any other takes the keys.
  const blank = tab?.url === "about:blank";
  useEffect(() => {
    if (focused && phase.kind === "live" && !blank) capture.current?.focus();
  }, [focused, phase.kind, blank]);

  const fail = useCallback((err: unknown) => {
    setError(err instanceof Error ? err.message : String(err));
  }, []);
  // One input at a time, in the order given: each waits for the tab to
  // take the one before it, and what piles up meanwhile is merged.
  const inputs = useRef<BrowserLiveInput[]>([]);
  const sending = useRef(false);
  const send = useCallback(
    (input: BrowserLiveInput) => {
      setError(null);
      queueLiveInput(inputs.current, input);
      if (sending.current) return;
      sending.current = true;
      void (async () => {
        for (
          let next = inputs.current.shift();
          next;
          next = inputs.current.shift()
        ) {
          await sendBrowserLiveInput(sessionId, tabId, next).catch(fail);
        }
        sending.current = false;
      })();
    },
    [sessionId, tabId, fail],
  );
  const navigate = useCallback(
    (nav: BrowserLiveNav) => {
      setError(null);
      void navigateBrowserLive(sessionId, tabId, nav).catch(fail);
    },
    [sessionId, tabId, fail],
  );
  // "New tab": a blank tab of the same thread, in this pane.
  const openTab = useCallback(() => {
    setError(null);
    void newBrowserTab(sessionId)
      .then((tab) =>
        requestBrowserPane({ sessionId, tab, title: "New tab", own: true }),
      )
      .catch(fail);
  }, [sessionId, fail]);
  const point = (event: { clientX: number; clientY: number }) => {
    const shown = surface.current?.getBoundingClientRect();
    const current = frame.current;
    if (!shown || !current) return null;
    return livePoint({ x: event.clientX, y: event.clientY }, shown, current);
  };
  const mouse = (
    kind: "mousePressed" | "mouseReleased" | "mouseMoved",
    event: MouseEvent,
  ) => {
    const at = point(event);
    if (!at) return;
    if (kind !== "mouseMoved") {
      // Keyboard input keeps going to the tab.
      event.preventDefault();
      capture.current?.focus();
    }
    // The press shows at once; the page's own answer follows in a picture.
    if (kind === "mousePressed" && stage.current)
      pressRipple(stage.current, event.clientX, event.clientY);
    send({
      kind: "mouse",
      event: kind,
      ...at,
      button:
        kind === "mouseMoved" ? "none" : (BUTTONS[event.button] ?? "left"),
      buttons: event.buttons,
      clickCount: kind === "mouseMoved" ? 0 : Math.max(event.detail, 1),
      modifiers: liveModifiers(event),
    });
  };
  const wheel = (event: WheelEvent) => {
    const at = point(event);
    if (!at) return;
    const dx = wheelPixels(
      event.deltaX,
      event.deltaMode,
      frame.current?.width ?? 0,
    );
    const dy = wheelPixels(
      event.deltaY,
      event.deltaMode,
      frame.current?.height ?? 0,
    );
    // As is: spreading a notch over frames, or moving the picture ahead
    // of the page, only put the pane further behind the browser.
    send({
      kind: "wheel",
      ...at,
      deltaX: dx,
      deltaY: dy,
      modifiers: liveModifiers(event),
    });
  };
  const key = (phase: "down" | "up", event: KeyboardEvent) => {
    if (picking && event.key === "Escape") {
      event.preventDefault();
      if (phase === "down") void cancelBrowserPick().catch(() => {});
      return;
    }
    if (isNewTabKey(event)) {
      event.preventDefault();
      if (phase === "down" && !event.repeat) openTab();
      return;
    }
    const input = liveKeyInput(event.nativeEvent, phase);
    if (!input) return;
    event.preventDefault();
    send(input);
  };
  // Typed, composed and pasted text arrives here as the text itself.
  const typed = (event: FormEvent<HTMLTextAreaElement>) => {
    const native = event.nativeEvent as InputEvent;
    const field = event.currentTarget;
    if (native.isComposing) return;
    if (field.value) send({ kind: "text", text: field.value });
    field.value = "";
  };

  const notice =
    !open && tab
      ? "This tab is no longer the thread's."
      : !tab
        ? "This tab was closed."
        : phase.kind === "gone"
          ? "This tab was closed."
          : phase.kind === "failed"
            ? phase.message
            : null;

  return (
    <div
      data-browser-live
      data-live-fps={fps}
      className="flex h-full min-h-0 flex-col bg-background-base"
    >
      <BrowserToolbar
        url={tab?.url ?? ""}
        busy={!!tab?.busy}
        disabled={!open}
        focusAddress={!!focused && blank}
        onNavigate={navigate}
        onNewTab={openTab}
      />
      {picking ? (
        <div className="flex h-8 shrink-0 items-center gap-2 border-b border-teal-500/20 bg-teal-400/10 px-3 text-[12px] text-teal-700 dark:text-teal-200">
          <span className="min-w-0 flex-1 truncate">
            Click an element to show it to the agent.
          </span>
          <button
            type="button"
            onClick={() => void cancelBrowserPick().catch(() => {})}
            className="h-6 shrink-0 rounded-md px-2 hover:bg-teal-400/15"
          >
            Cancel
          </button>
        </div>
      ) : null}
      <div
        ref={stage}
        role="application"
        aria-label={`Browser tab ${tab?.title || "Untitled"}`}
        onMouseDown={(event) => mouse("mousePressed", event)}
        onMouseUp={(event) => mouse("mouseReleased", event)}
        onMouseMove={(event) => mouse("mouseMoved", event)}
        onWheel={wheel}
        onContextMenu={(event) => event.preventDefault()}
        className="relative min-h-0 flex-1 overflow-hidden"
      >
        <textarea
          ref={capture}
          aria-label="Type into the browser tab"
          autoCapitalize="off"
          autoComplete="off"
          autoCorrect="off"
          spellCheck={false}
          onKeyDown={(event) => key("down", event)}
          onKeyUp={(event) => key("up", event)}
          onInput={typed}
          onCompositionEnd={(event) => {
            const field = event.currentTarget;
            if (event.data) send({ kind: "text", text: event.data });
            field.value = "";
          }}
          className="pointer-events-none absolute top-0 left-0 size-px resize-none opacity-0"
        />
        {/* At the picture's own pixels, one per device pixel, never
            stretched to the pane (that blurs text); sized as it is drawn. */}
        <div
          ref={surface}
          aria-hidden
          className={`absolute top-0 left-0 max-w-none select-none transition-opacity duration-150 ${
            shown ? "" : "invisible"
          } ${phase.kind === "live" && !notice ? "" : "opacity-40"}`}
        />
        {notice || phase.kind === "elsewhere" ? (
          <div className="absolute inset-0 grid place-items-center p-6">
            <div className="flex max-w-xs flex-col items-center gap-3 rounded-xl border border-content/10 bg-background-base/95 px-5 py-4 text-center text-[12px] text-content/65 shadow-sm">
              <Globe className="size-4 text-content/40" strokeWidth={1.75} />
              <p>{notice ?? "This tab is showing in another pane."}</p>
              {!notice ? (
                <button
                  type="button"
                  onClick={() => setAttempt((n) => n + 1)}
                  className="h-7 rounded-md bg-content/8 px-3 text-content hover:bg-content/12"
                >
                  Show here
                </button>
              ) : null}
            </div>
          </div>
        ) : !shown ? (
          <div className="absolute inset-0 grid place-items-center">
            <p className="flex items-center gap-2 text-[12px] text-content/45">
              <span className="size-1.5 animate-pulse rounded-full bg-teal-500" />
              Connecting to the tab…
            </p>
          </div>
        ) : null}
        {error ? (
          <p
            role="status"
            className="absolute right-3 bottom-3 left-3 rounded-lg border border-content/10 bg-background-base/95 px-3 py-2 text-[12px] text-content/70 shadow-sm"
          >
            {error}
          </p>
        ) : null}
      </div>
    </div>
  );
}

const TOOL =
  "grid size-7 shrink-0 place-items-center rounded-md text-content/55 hover:bg-content/8 hover:text-content disabled:pointer-events-none disabled:opacity-35";

/** Back, Forward, Reload, the address and what the agent is doing. */
function BrowserToolbar({
  url,
  busy,
  disabled,
  focusAddress,
  onNavigate,
  onNewTab,
}: {
  url: string;
  busy: boolean;
  disabled: boolean;
  /** The address field takes the keys (a blank tab, waiting for one). */
  focusAddress: boolean;
  onNavigate: (nav: BrowserLiveNav) => void;
  onNewTab: () => void;
}) {
  const [editing, setEditing] = useState<string | null>(null);
  const field = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (focusAddress && !disabled) field.current?.focus();
  }, [focusAddress, disabled]);
  const address = liveAddress(url);
  return (
    <div className="flex h-10 shrink-0 items-center gap-0.5 border-b border-content/8 px-1.5">
      <button
        type="button"
        title="Back"
        aria-label="Back"
        disabled={disabled}
        onClick={() => onNavigate({ kind: "back" })}
        className={TOOL}
      >
        <ChevronLeft className="size-4" strokeWidth={1.75} />
      </button>
      <button
        type="button"
        title="Forward"
        aria-label="Forward"
        disabled={disabled}
        onClick={() => onNavigate({ kind: "forward" })}
        className={TOOL}
      >
        <ChevronRight className="size-4" strokeWidth={1.75} />
      </button>
      <button
        type="button"
        title="Reload"
        aria-label="Reload"
        disabled={disabled}
        onClick={() => onNavigate({ kind: "reload" })}
        className={TOOL}
      >
        <RefreshCw className="size-3.5" strokeWidth={1.75} />
      </button>
      <form
        className="mx-1 min-w-0 flex-1"
        onSubmit={(event) => {
          event.preventDefault();
          const typed = editing?.trim();
          setEditing(null);
          field.current?.blur();
          if (typed && typed !== url) onNavigate({ kind: "go", url: typed });
        }}
      >
        <label className="relative flex h-7 min-w-0 items-center gap-1.5 rounded-md bg-content/5 px-2.5 text-[12px] focus-within:bg-content/8">
          {address.secure ? (
            <Lock
              className="size-3 shrink-0 text-content/40"
              strokeWidth={1.75}
            />
          ) : (
            <Globe
              className="size-3 shrink-0 text-content/40"
              strokeWidth={1.75}
            />
          )}
          {editing === null ? (
            <span className="pointer-events-none min-w-0 flex-1 truncate">
              <span className="text-content">{address.host}</span>
              <span className="text-content/45">{address.rest}</span>
            </span>
          ) : null}
          <input
            ref={field}
            aria-label="Address"
            disabled={disabled}
            spellCheck={false}
            value={editing ?? ""}
            onFocus={(event) => {
              setEditing(url);
              const input = event.currentTarget;
              requestAnimationFrame(() => input.select());
            }}
            onBlur={() => setEditing(null)}
            onChange={(event) => setEditing(event.currentTarget.value)}
            onKeyDown={(event) => {
              if (event.key !== "Escape") return;
              setEditing(null);
              event.currentTarget.blur();
            }}
            className={
              editing === null
                ? "absolute inset-0 cursor-text bg-transparent opacity-0 outline-none"
                : "min-w-0 flex-1 bg-transparent text-content outline-none"
            }
          />
        </label>
      </form>
      {busy ? (
        <span
          className="mr-1 flex h-6 shrink-0 items-center gap-1.5 rounded-full bg-teal-400/12 px-2 text-[11px] text-teal-700 dark:text-teal-300"
          title="The agent is acting in this tab"
        >
          <span className="size-1.5 animate-pulse rounded-full bg-teal-500" />
          Agent
        </span>
      ) : null}
      <button
        type="button"
        title="New tab (Ctrl+T)"
        aria-label="New tab"
        onClick={onNewTab}
        className={TOOL}
      >
        <Plus className="size-4" strokeWidth={1.75} />
      </button>
    </div>
  );
}
