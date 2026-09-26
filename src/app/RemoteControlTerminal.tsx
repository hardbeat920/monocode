import { Terminal } from "@xterm/xterm";
import { useEffect, useRef } from "react";
import { writePty } from "../platform/tauri/pty";
import "@xterm/xterm/css/xterm.css";

/**
 * The remote-control pty, in front of the user, when MonoCode cannot answer for
 * them.
 *
 * Deliberately not `TerminalView`. That component owns a pty's whole life — it
 * spawns one, resizes it to fit its pane, and kills it on unmount — and all
 * three are wrong here. The pty already exists, it is spawned at a fixed size the
 * screen parser depends on, and it has to outlive this view.
 *
 * It also cannot subscribe: `subscribePty` keeps **one** handler per id, so a
 * second subscriber would replace the parser's and its unsubscribe would leave
 * the pty unwatched. So output arrives through `attach`, from the single
 * subscription App.tsx already owns, and this view is one more reader of it.
 */
export function RemoteControlTerminal({
  ptyId,
  replay,
  attach,
  cols,
  rows,
  onClose,
}: {
  ptyId: string;
  /** What is already on screen, so the view opens showing the prompt. */
  replay: string;
  /** Registers a live reader; the returned function deregisters it. */
  attach: (read: (chunk: string) => void) => () => void;
  cols: number;
  rows: number;
  onClose: () => void;
}) {
  const hostRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    // Fixed to the size the pty was spawned at. Never fitted to the pane and
    // never resized through the pty: every column the parser reads is placed by
    // absolute cursor moves, so a resize would silently move all of them.
    const term = new Terminal({
      cols,
      rows,
      cursorBlink: true,
      cursorStyle: "bar",
      fontSize: 13,
      lineHeight: 1,
      scrollback: 1000,
      allowTransparency: true,
      smoothScrollDuration: 0,
    });
    term.open(host);
    term.write(replay);
    term.focus();

    const detach = attach((chunk) => term.write(chunk));
    const typing = term.onData((data) => {
      void writePty(ptyId, data).catch(() => undefined);
    });

    return () => {
      typing.dispose();
      detach();
      // The pty is not ours to end. Closing this view puts the screen away;
      // closing remote control is what kills the process.
      term.dispose();
    };
  }, [attach, cols, ptyId, replay, rows]);

  return (
    <div className="flex min-h-0 flex-col gap-1 border-b border-content/10 px-2 py-2">
      <div className="flex items-center gap-2 text-[12px]">
        <span className="text-content/45">
          Remote Control could not read this screen — answer it here
        </span>
        <button
          type="button"
          onClick={onClose}
          className="ml-auto rounded px-1.5 py-0.5 text-content/55 hover:bg-content/10 hover:text-content"
        >
          Hide
        </button>
      </div>
      <div ref={hostRef} className="min-h-0 overflow-auto" />
    </div>
  );
}
