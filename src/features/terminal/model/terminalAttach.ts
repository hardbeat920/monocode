/** The part of an xterm `Terminal` the attach gate drives. */
export type AttachTerminal = {
  write(data: string | Uint8Array, callback?: () => void): void;
  options: { disableStdin?: boolean };
};

export type AttachGate = {
  /** PTY output: held until `open`, then passed straight to `deliver`. */
  data(chunk: Uint8Array): void;
  /** `pty_spawn` answered. `restore` is empty for a fresh shell. */
  open(restore: string): void;
  /** The view is going away; never re-enable its input. */
  close(): void;
};

/**
 * Holds a terminal view's output and input until `pty_spawn` answers.
 *
 * A reattached PTY is already running a program that switched on modes a
 * fresh xterm knows nothing about (bracketed paste, the alternate screen,
 * application cursor keys), and shells and Vim do not resend them on
 * SIGWINCH. Those modes go in first; the view takes keys and pastes only once
 * they are parsed, so even the first paste is wrapped the way the program
 * expects. Output that arrived meanwhile follows, after input is back on, so
 * the view can answer any terminal queries in it.
 */
export function createAttachGate(
  term: AttachTerminal,
  deliver: (chunk: Uint8Array) => void,
): AttachGate {
  let opened = false;
  let closed = false;
  let pending: Uint8Array[] = [];
  term.options.disableStdin = true;
  return {
    data(chunk) {
      if (opened) deliver(chunk);
      else pending.push(chunk);
    },
    open(restore) {
      if (opened || closed) return;
      opened = true;
      // xterm parses writes in order and runs each callback right after its
      // own write, so input comes back before the held output is parsed.
      term.write(restore, () => {
        if (!closed) term.options.disableStdin = false;
      });
      const held = pending;
      pending = [];
      for (const chunk of held) deliver(chunk);
    },
    close() {
      closed = true;
      pending = [];
    },
  };
}
