type TerminalKeyEvent = Pick<
  KeyboardEvent,
  "key" | "altKey" | "ctrlKey" | "metaKey" | "shiftKey"
>;

/** Translate macOS editing shortcuts into sequences understood by common shells. */
export function macTerminalShortcutData(
  event: TerminalKeyEvent,
): string | null {
  if (event.ctrlKey || event.shiftKey) return null;

  if (event.altKey && !event.metaKey) {
    if (event.key === "ArrowLeft") return "\x1bb";
    if (event.key === "ArrowRight") return "\x1bf";
    return null;
  }

  if (event.metaKey && !event.altKey) {
    if (event.key === "ArrowLeft") return "\x01";
    if (event.key === "ArrowRight") return "\x05";
    if (event.key === "Backspace") return "\x15";
  }

  return null;
}
