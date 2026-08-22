import { Terminal } from "@xterm/xterm";
import { useEffect, useRef } from "react";
import {
  getPtyStatus,
  killPty,
  resizePty,
  spawnPty,
  subscribePty,
  writePty,
} from "../lib/pty";
import { isOscColorQuery, oscColorReply } from "../lib/terminalChrome";
import {
  defaultTerminalTitle,
  scanOscCwd,
  type TerminalMetaPatch,
} from "../lib/terminalTab";
import { isLightScheme, SCHEME_CHANGE_EVENT } from "../lib/appearance";
import {
  applyTerminalChrome,
  fitTerminal,
  resetGridStretch,
  type TerminalFitMode,
} from "../lib/terminalLayout";
import { IS_MAC } from "../lib/platform";
import "@xterm/xterm/css/xterm.css";

type Props = {
  id: string;
  cwd: string;
  active: boolean;
  onMetaChange?: (patch: TerminalMetaPatch) => void;
};

function cssColor(expr: string, fallback: string): string {
  const probe = document.createElement("span");
  probe.style.color = expr;
  document.body.appendChild(probe);
  const color = getComputedStyle(probe).color;
  probe.remove();
  return color || fallback;
}

const ANSI_DARK = {
  black: "#1d2428",
  red: "#f87171",
  green: "#4ade80",
  yellow: "#fbbf24",
  blue: "#60a5fa",
  magenta: "#c084fc",
  cyan: "#22d3ee",
  white: "#e8eef2",
  brightBlack: "#64748b",
  brightRed: "#fca5a5",
  brightGreen: "#86efac",
  brightYellow: "#fde68a",
  brightBlue: "#93c5fd",
  brightMagenta: "#d8b4fe",
  brightCyan: "#67e8f9",
  brightWhite: "#f8fafc",
};

// One-Light-family palette tuned for a near-white canvas.
const ANSI_LIGHT = {
  black: "#383a42",
  red: "#e45649",
  green: "#50a14f",
  yellow: "#c18401",
  blue: "#4078f2",
  magenta: "#a626a4",
  cyan: "#0184bc",
  white: "#fafafa",
  brightBlack: "#7c8591",
  brightRed: "#df6b60",
  brightGreen: "#68b567",
  brightYellow: "#d19a2f",
  brightBlue: "#5c89f5",
  brightMagenta: "#b54bb3",
  brightCyan: "#1f9cc9",
  brightWhite: "#ffffff",
};

function terminalTheme(light: boolean) {
  return {
    background: "#00000000",
    foreground: cssColor("var(--color-content)", light ? "#2e2e2e" : "#e8eef2"),
    cursor: cssColor("var(--color-accent)", light ? "#4078f2" : "#4da3f5"),
    cursorAccent: light ? "#ffffff" : "#000000",
    selectionBackground: light
      ? "rgba(0,0,0,0.18)"
      : "rgba(255,255,255,0.18)",
    selectionInactiveBackground: light
      ? "rgba(0,0,0,0.08)"
      : "rgba(255,255,255,0.08)",
    ...(light ? ANSI_LIGHT : ANSI_DARK),
  };
}

function monoFont(): string {
  const fromCss = getComputedStyle(document.documentElement)
    .getPropertyValue("--font-mono")
    .trim();
  return fromCss || "ui-monospace, SFMono-Regular, Menlo, Monaco, monospace";
}

// OSC 10/11/12 replies so CLIs (vim, tmux, …) pick matching colors.
const OSC_DARK = { fg: "#e8eef2", bg: "#141b1f", cursor: "#4da3f5" };
const OSC_LIGHT = { fg: "#383a42", bg: "#fafafa", cursor: "#4078f2" };

function oscColors() {
  return isLightScheme() ? OSC_LIGHT : OSC_DARK;
}

export function TerminalView({ id, cwd, active, onMetaChange }: Props) {
  const outerRef = useRef<HTMLDivElement>(null);
  const hostRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const spawned = useRef(false);
  const applySizeRef = useRef<() => void>(() => {});
  const onMetaChangeRef = useRef(onMetaChange);
  onMetaChangeRef.current = onMetaChange;
  const runningProcessRef = useRef<string | null>(null);

  useEffect(() => {
    const outer = outerRef.current;
    const host = hostRef.current;
    if (!outer || !host) return;

    const term = new Terminal({
      cursorBlink: true,
      cursorStyle: "bar",
      fontFamily: monoFont(),
      fontSize: 13,
      lineHeight: 1,
      letterSpacing: 0,
      scrollback: 5000,
      allowTransparency: true,
      smoothScrollDuration: 0,
      theme: terminalTheme(isLightScheme()),
      macOptionIsMeta: IS_MAC,
    });
    term.open(host);
    termRef.current = term;
    let closed = false;

    const onCopy = (event: ClipboardEvent) => {
      const text = term.getSelection();
      if (!text) return;
      event.clipboardData?.setData("text/plain", text);
      event.preventDefault();
    };
    const onPaste = (event: ClipboardEvent) => {
      const text = event.clipboardData?.getData("text/plain");
      if (!text) return;
      event.preventDefault();
      term.paste(text);
    };
    host.addEventListener("copy", onCopy);
    host.addEventListener("paste", onPaste);

    term.attachCustomKeyEventHandler((event) => {
      const mod = event.metaKey || event.ctrlKey;
      if (!mod || event.altKey) return true;
      const key = event.key.toLowerCase();
      if (key === "c") {
        if (term.hasSelection()) return false;
        if (event.metaKey && !event.ctrlKey) return false;
        return true;
      }
      if (key === "v") return false;
      return true;
    });

    let oscBuffer = "";

    const unsubscribe = subscribePty(
      id,
      (data) => {
        const onMeta = onMetaChangeRef.current;
        if (onMeta) {
          const text = new TextDecoder().decode(data);
          const scanned = scanOscCwd(text, oscBuffer);
          oscBuffer = scanned.rest;
          if (scanned.cwd) {
            const patch: TerminalMetaPatch = { cwd: scanned.cwd };
            if (!runningProcessRef.current) {
              patch.title = defaultTerminalTitle(scanned.cwd);
            }
            onMeta(patch);
          }
        }
        term.write(data);
      },
      (code) => {
        if (closed) return;
        const status = code == null ? "" : ` (${code})`;
        term.writeln(`\r\n[process exited${status}]`);
      },
    );

    const dataSub = term.onData((data) => {
      void writePty(id, data);
    });

    const replyOsc = (code: 10 | 11 | 12, hex: string) => {
      const reply = oscColorReply(code, hex);
      if (reply) void writePty(id, reply);
      return true;
    };
    const oscFg = term.parser.registerOscHandler(10, (data) =>
      isOscColorQuery(data) ? replyOsc(10, oscColors().fg) : false,
    );
    const oscBg = term.parser.registerOscHandler(11, (data) =>
      isOscColorQuery(data) ? replyOsc(11, oscColors().bg) : false,
    );
    const oscCursor = term.parser.registerOscHandler(12, (data) =>
      isOscColorQuery(data) ? replyOsc(12, oscColors().cursor) : false,
    );

    const onSchemeChange = () => {
      term.options.theme = terminalTheme(isLightScheme());
    };
    window.addEventListener(SCHEME_CHANGE_EVENT, onSchemeChange);

    term.attachCustomWheelEventHandler(() => {
      if (term.element?.classList.contains("enable-mouse-events")) return true;
      return term.buffer.active.type !== "alternate";
    });

    let lastCols = 0;
    let lastRows = 0;
    let raf = 0;
    let tuiMode = false;

    const fitMode = (): TerminalFitMode =>
      term.buffer.active.type === "alternate" ? "tui" : "shell";

    const syncAltScreenMode = () => {
      const next = fitMode() === "tui";
      if (next === tuiMode) return;
      tuiMode = next;
      applyTerminalChrome(term, outer, next);
      if (!next) resetGridStretch(term);
      lastCols = 0;
      lastRows = 0;
      schedule();
    };

    const applySize = () => {
      const next = fitTerminal(term, host, fitMode());
      if (!next) return;
      const { cols, rows } = next;
      if (cols === lastCols && rows === lastRows) return;
      lastCols = cols;
      lastRows = rows;
      if (!spawned.current) {
        spawned.current = true;
        void spawnPty(id, cwd, cols, rows).catch((error) => {
          spawned.current = false;
          lastCols = 0;
          lastRows = 0;
          const message =
            error instanceof Error ? error.message : String(error);
          term.writeln(`\x1b[31m${message}\x1b[0m`);
        });
        return;
      }
      void resizePty(id, cols, rows);
    };

    const schedule = () => {
      if (raf) return;
      raf = requestAnimationFrame(() => {
        raf = 0;
        applySize();
      });
    };

    applySizeRef.current = applySize;
    const renderSub = term.onRender(() => {
      if (!spawned.current) applySize();
    });
    const bufferSub = term.buffer.onBufferChange(syncAltScreenMode);
    syncAltScreenMode();
    const frame = requestAnimationFrame(applySize);
    const observer = new ResizeObserver(schedule);
    observer.observe(host);

    return () => {
      closed = true;
      cancelAnimationFrame(frame);
      if (raf) cancelAnimationFrame(raf);
      observer.disconnect();
      outer.classList.remove("monocode-terminal--alt-screen");
      applySizeRef.current = () => {};
      host.removeEventListener("copy", onCopy);
      host.removeEventListener("paste", onPaste);
      window.removeEventListener(SCHEME_CHANGE_EVENT, onSchemeChange);
      dataSub.dispose();
      oscFg.dispose();
      oscBg.dispose();
      oscCursor.dispose();
      renderSub.dispose();
      bufferSub.dispose();
      unsubscribe();
      void killPty(id);
      term.dispose();
      termRef.current = null;
      spawned.current = false;
    };
  }, [id, cwd]);

  useEffect(() => {
    if (!onMetaChange) return;
    let lastForeground: string | null = null;
    const refresh = () => {
      if (!spawned.current) return;
      void getPtyStatus(id)
        .then(({ foreground }) => {
          const fg = foreground?.trim() || null;
          runningProcessRef.current = fg;
          if (fg === lastForeground) return;
          lastForeground = fg;
          onMetaChangeRef.current?.(
            fg ? { title: fg } : { title: defaultTerminalTitle(cwd) },
          );
        })
        .catch(() => undefined);
    };
    refresh();
    const interval = setInterval(refresh, 1000);
    return () => clearInterval(interval);
  }, [id, cwd, onMetaChange]);

  useEffect(() => {
    if (!active) return;
    applySizeRef.current();
    termRef.current?.focus();
  }, [active]);

  return (
    <div
      ref={outerRef}
      className="monocode-terminal flex h-full w-full min-h-0 min-w-0 flex-col"
      onMouseDown={() => termRef.current?.focus()}
    >
      <div
        ref={hostRef}
        className="monocode-terminal-host min-h-0 min-w-0 flex-1 overflow-hidden"
      />
    </div>
  );
}
