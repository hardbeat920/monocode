/** The same spelling is accepted by tauri-plugin-global-shortcut. */
export const QUICK_COMPOSER_DEFAULT_SHORTCUT = "Command+Shift+Space";

type Modifiers = Pick<
  KeyboardEvent,
  "metaKey" | "ctrlKey" | "altKey" | "shiftKey"
>;

const SYMBOLS: Record<string, string> = {
  Space: "Space",
  Backquote: "`",
  Backslash: "\\",
  BracketLeft: "[",
  BracketRight: "]",
  Comma: ",",
  Equal: "=",
  Minus: "-",
  Period: ".",
  Quote: "'",
  Semicolon: ";",
  Slash: "/",
  ArrowUp: "↑",
  ArrowDown: "↓",
  ArrowLeft: "←",
  ArrowRight: "→",
  Enter: "Return",
  NumpadEnter: "Return",
  Tab: "Tab",
  Backspace: "Delete",
  Delete: "Delete Forward",
  Home: "Home",
  End: "End",
  PageUp: "Page Up",
  PageDown: "Page Down",
};

function supportedCode(code: string): boolean {
  return (
    /^Key[A-Z]$/.test(code) ||
    /^Digit[0-9]$/.test(code) ||
    /^F(?:[1-9]|1[0-9]|2[0-4])$/.test(code) ||
    Object.prototype.hasOwnProperty.call(SYMBOLS, code)
  );
}

export function isQuickComposerShortcut(value: string): boolean {
  const parts = value.split("+");
  const code = parts.pop();
  if (!code || !supportedCode(code)) return false;
  if (!parts.some((part) => ["Command", "Control"].includes(part)))
    return false;
  return (
    parts.length > 0 &&
    parts.length <= 4 &&
    new Set(parts).size === parts.length &&
    parts.every((part) =>
      ["Command", "Control", "Option", "Shift"].includes(part),
    )
  );
}

export function shortcutFromKeyEvent(
  event: Pick<KeyboardEvent, "code"> & Modifiers,
): string | null {
  if (!supportedCode(event.code)) return null;
  const modifiers = [
    event.metaKey && "Command",
    event.ctrlKey && "Control",
    event.altKey && "Option",
    event.shiftKey && "Shift",
  ].filter(Boolean);
  if (!event.metaKey && !event.ctrlKey) return null;
  return [...modifiers, event.code].join("+");
}

function codeLabel(code: string): string {
  return code.startsWith("Key")
    ? code.slice(3)
    : code.startsWith("Digit")
      ? code.slice(5)
      : (SYMBOLS[code] ?? code);
}

export function quickComposerShortcutPreview(
  modifiers: Modifiers,
  code?: string,
  key?: string,
): string {
  const prefix = [
    modifiers.metaKey && "⌘",
    modifiers.ctrlKey && "⌃",
    modifiers.altKey && "⌥",
    modifiers.shiftKey && "⇧",
  ]
    .filter(Boolean)
    .join("");
  if (!code) return prefix;
  const displayedKey = supportedCode(code)
    ? codeLabel(code)
    : key && key !== "Unidentified"
      ? key.length === 1
        ? key.toUpperCase()
        : key
      : "";
  return prefix + displayedKey;
}

export function quickComposerShortcutLabel(value: string): string {
  const parts = value.split("+");
  const code = parts.pop() ?? "Space";
  return (
    parts
      .map(
        (part) =>
          ({ Command: "⌘", Control: "⌃", Option: "⌥", Shift: "⇧" })[
            part as "Command" | "Control" | "Option" | "Shift"
          ],
      )
      .join("") + codeLabel(code)
  );
}
