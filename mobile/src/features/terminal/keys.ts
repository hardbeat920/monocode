import { textBytes } from "./base64";

export type TerminalKey = {
  label: string;
  text: string;
  name: string;
  /** Held down, offers these instead, slid onto as the keyboard's accents are. */
  more?: TerminalKey[];
};

const ctrlKey = (letter: string, name: string): TerminalKey => ({
  label: `^${letter}`,
  text: String.fromCharCode(letter.charCodeAt(0) & 0x1f),
  name: `${name} (Control ${letter})`,
});
const symbol = (text: string, name: string): TerminalKey => ({ label: text, text, name });

export const ESC: TerminalKey = { label: "esc", text: "\x1b", name: "Escape" };
export const TAB: TerminalKey = { label: "tab", text: "\t", name: "Tab" };

/** ^C, and held, the other control keys a shell is worked with. */
export const INTERRUPT: TerminalKey = {
  ...ctrlKey("C", "Interrupt"),
  more: [
    ctrlKey("D", "End of input"),
    ctrlKey("Z", "Suspend"),
    ctrlKey("L", "Clear screen"),
    ctrlKey("R", "Search history"),
    ctrlKey("A", "Start of line"),
    ctrlKey("E", "End of line"),
    ctrlKey("U", "Delete to start of line"),
    ctrlKey("W", "Delete word"),
  ],
};

/** /, and held, the symbols the phone keyboard hides a layer down. */
export const SLASH: TerminalKey = {
  ...symbol("/", "Slash"),
  more: [
    symbol("~", "Tilde"),
    symbol("|", "Pipe"),
    symbol("-", "Dash"),
    symbol("\\", "Backslash"),
    symbol("`", "Backtick"),
    symbol("$", "Dollar"),
    symbol("*", "Asterisk"),
    symbol("&", "Ampersand"),
    symbol(">", "Greater than"),
  ],
};

export type Arrow = "up" | "down" | "left" | "right";
export const ARROWS: Record<Arrow, string> = {
  up: "\x1b[A",
  down: "\x1b[B",
  right: "\x1b[C",
  left: "\x1b[D",
};

/** What Ctrl does to a typed letter: the control code, as ^C is 3. Others pass through. */
export function withCtrl(bytes: Uint8Array): Uint8Array {
  if (bytes.length !== 1) return bytes;
  const code = bytes[0];
  if (code >= 0x61 && code <= 0x7a) return Uint8Array.of(code & 0x1f);
  if (code >= 0x40 && code <= 0x5f) return Uint8Array.of(code & 0x1f);
  if (code === 0x20) return Uint8Array.of(0);
  return bytes;
}

/** A saved action, typed and run. */
export function actionBytes(command: string): Uint8Array {
  return textBytes(`${command}\r`);
}
