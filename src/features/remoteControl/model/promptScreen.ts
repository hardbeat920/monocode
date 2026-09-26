/**
 * What the interactive Claude TUI has on screen, read off the pty.
 *
 * Permission prompts and turn ends are not written to the transcript
 * (`docs/remote-control.md` §7 and §8), so the rendered screen is the only
 * source for either, and a modal swallows injected text silently (§5). This
 * module answers the three questions that follow from that — is a prompt
 * pending, may a message be injected, is a turn running — from rendered bytes
 * alone. No pty, no I/O, no timers.
 *
 * The rule it exists to enforce: recognise a screen or say so. Anything that
 * does not match a screen seen in a real session is `unrecognised`, which
 * refuses injection and carries no answer. Keystrokes exist only as fields of
 * options parsed off a recognised permission prompt, so there is no way to
 * derive an answer from a screen this module did not understand — including the
 * trust modal, whose options are deliberately returned without keystrokes.
 */

export type ScreenSize = { cols: number; rows: number };

/** Whether a turn is running. `ended` means "not running", so it clears a pending state. */
export type TurnState = "in-progress" | "ended" | "unknown";

export type PromptOption = {
  /** The number the TUI printed, 1-based. */
  number: number;
  label: string;
  /**
   * What to write into the pty to take this option. The digit is the key the
   * TUI advertises by numbering the option; `\r` was the one measured, taking
   * whichever option the cursor sits on.
   */
  keystroke: string;
  /** The option under the TUI's `❯` cursor, which is what `\r` would take. */
  selected: boolean;
};

export type PermissionPrompt = {
  /** The question line, verbatim: `Do you want to create probe.txt?` */
  question: string;
  /** What the prompt box shows above the question: the tool header and its diff. */
  detail: readonly string[];
  options: readonly PromptOption[];
  /** The prompt's own footer, verbatim: `Esc to cancel · Tab to amend`. */
  footer: string;
  /** Present when the footer advertises Esc, which dismisses without answering. */
  cancel?: { label: string; keystroke: string };
};

/** The one modal seen in a real session: the first-run folder trust check. */
export type RecognisedModal = {
  modal: "trust-folder";
  title: string;
  /** The modal's options as text. No keystrokes: MonoCode must not answer this. */
  options: readonly string[];
};

export type PromptScreen = {
  /** The rendered screen, so a caller that has to give up can show it. */
  lines: readonly string[];
  turn: TurnState;
} & (
  | { kind: "permission-prompt"; prompt: PermissionPrompt }
  | { kind: "modal"; modal: RecognisedModal }
  | { kind: "idle" }
  | { kind: "busy" }
  | { kind: "unrecognised"; reason: UnrecognisedReason }
);

export type UnrecognisedReason =
  /** Numbered options are on screen but the dialog around them is not known. */
  | "unknown-dialog"
  /** No composer, no dialog — a banner, a partial repaint, or something new. */
  | "no-composer";

export type InjectionRefusal =
  | "empty"
  | "permission-prompt"
  | "modal"
  | "unrecognised-screen"
  /** `/`, `!` and `#` drive the TUI's own affordances instead of sending text (§5). */
  | "command-prefix";

export type InjectionPlan =
  | {
      allowed: true;
      /** Bracketed paste and a return, the sequence measured in §5. */
      bytes: string;
      /** The TUI queues a message sent mid-turn; it arrives when the turn ends. */
      queued: boolean;
    }
  | { allowed: false; reason: InjectionRefusal };

const RULE = /^[─━]{8,}$/u;
const OPTION = /^(❯\s*)?(\d+)\.\s+(\S.*)$/u;
const QUESTION_START = /^Do you want to /u;
const QUESTION = /^Do you want to .+\?$/u;
const PROMPT_FOOTER = /Esc to cancel/u;
const COMPOSER = /^❯(?:\s|$)/u;
/** A spinner frame: a glyph, then one word ending in an ellipsis. */
const SPINNER = /^[^\p{L}\p{N}\s]\s+\S+…/u;
/** The line a finished turn leaves behind: `✻ Baked for 2s`. */
const COMPLETED = /\bfor \d+(?:\.\d+)?s$/u;
const TRUST_QUESTION = /Is this a project you created or one you trust\?/u;
const TRUST_HEADER = /^Accessing workspace:$/u;
const TRUST_FOOTER = /Enter to confirm/u;

const CSI = /\x1b\[([0-9;:<=>?]*)[ -/]*([@-~])/y;
const OSC = /\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)/y;
const CHARSET = /\x1b[()*+][0-9A-Za-z]/y;
/** An escape cut in half by the end of the chunk; the rest is in the next one. */
const PARTIAL = /^\x1b(?:\[[0-9;:<=>?]*[ -/]*|\][^\x07\x1b]*|[()*+]?)?$/;

/**
 * The screen the TUI has painted, one string per row.
 *
 * A quarter of a terminal emulator: enough of it that the text lands where the
 * TUI put it. The CLI paints words at absolute columns rather than writing
 * spaces, so stripping escapes instead of placing cells runs `Do you want to`
 * together into `Doyouwantto` and loses every column the layout depends on.
 */
export function renderScreen(output: string, size: ScreenSize): string[] {
  const cols = Math.max(1, Math.trunc(size.cols));
  const rows = Math.max(1, Math.trunc(size.rows));
  const grid: string[][] = [];
  for (let i = 0; i < rows; i += 1)
    grid.push(new Array<string>(cols).fill(" "));
  let row = 0;
  let col = 0;
  let saved = { row: 0, col: 0 };

  const clear = (target: number, from: number, to: number) => {
    for (let x = from; x <= to && x < cols; x += 1) grid[target][x] = " ";
  };
  const lineFeed = () => {
    if (row + 1 < rows) {
      row += 1;
      return;
    }
    grid.shift();
    grid.push(new Array<string>(cols).fill(" "));
  };

  let i = 0;
  while (i < output.length) {
    const ch = output[i];
    if (ch === "\x1b") {
      if (PARTIAL.test(output.slice(i))) break;
      CSI.lastIndex = i;
      const csi = CSI.exec(output);
      if (csi) {
        const params = csi[1]
          .split(";")
          .map((part) => (/^\d+$/.test(part) ? Number(part) : 0));
        const first = params[0] ?? 0;
        const count = Math.max(1, first);
        switch (csi[2]) {
          case "H":
          case "f":
            row = Math.min(rows - 1, Math.max(0, (params[0] || 1) - 1));
            col = Math.min(cols - 1, Math.max(0, (params[1] || 1) - 1));
            break;
          case "G":
            col = Math.min(cols - 1, Math.max(0, (first || 1) - 1));
            break;
          case "d":
            row = Math.min(rows - 1, Math.max(0, (first || 1) - 1));
            break;
          case "A":
            row = Math.max(0, row - count);
            break;
          case "B":
            row = Math.min(rows - 1, row + count);
            break;
          case "C":
            col = Math.min(cols - 1, col + count);
            break;
          case "D":
            col = Math.max(0, col - count);
            break;
          case "K":
            if (first === 1) clear(row, 0, col);
            else if (first === 2) clear(row, 0, cols - 1);
            else clear(row, col, cols - 1);
            break;
          case "J":
            if (first === 1) {
              for (let y = 0; y < row; y += 1) clear(y, 0, cols - 1);
              clear(row, 0, col);
            } else if (first === 2 || first === 3) {
              for (let y = 0; y < rows; y += 1) clear(y, 0, cols - 1);
            } else {
              clear(row, col, cols - 1);
              for (let y = row + 1; y < rows; y += 1) clear(y, 0, cols - 1);
            }
            break;
          default:
            break;
        }
        i = CSI.lastIndex;
        continue;
      }
      OSC.lastIndex = i;
      const osc = OSC.exec(output);
      if (osc) {
        i = OSC.lastIndex;
        continue;
      }
      CHARSET.lastIndex = i;
      const charset = CHARSET.exec(output);
      if (charset) {
        i = CHARSET.lastIndex;
        continue;
      }
      const next = output[i + 1];
      if (next === "7") saved = { row, col };
      else if (next === "8") ({ row, col } = saved);
      else if (next === "M") row = Math.max(0, row - 1);
      else if (next === "D" || next === "E") lineFeed();
      i += 2;
      continue;
    }
    if (ch === "\r") {
      col = 0;
      i += 1;
      continue;
    }
    if (ch === "\n") {
      lineFeed();
      i += 1;
      continue;
    }
    if (ch === "\b") {
      col = Math.max(0, col - 1);
      i += 1;
      continue;
    }
    if (ch < " " || ch === "\x7f") {
      i += 1;
      continue;
    }
    const point = output.codePointAt(i);
    const glyph = String.fromCodePoint(point ?? 32);
    if (col >= cols) {
      col = 0;
      lineFeed();
    }
    grid[row][col] = glyph;
    col += 1;
    i += glyph.length;
  }

  return grid.map((line) => line.join("").replace(/\s+$/u, ""));
}

/** Read the screen the TUI has painted in `output`. */
export function readPromptScreen(
  output: string,
  size: ScreenSize,
): PromptScreen {
  return readRenderedScreen(renderScreen(output, size));
}

/** Read an already-rendered screen, one string per row. */
export function readRenderedScreen(lines: readonly string[]): PromptScreen {
  const trimmed = lines.map((line) => line.trim());

  const prompt = readPermissionPrompt(lines, trimmed);
  // A pending prompt means the turn is still running: the transcript stayed
  // silent for the 36 seconds one sat on screen (§7).
  if (prompt)
    return { kind: "permission-prompt", prompt, lines, turn: "in-progress" };

  const modal = readTrustModal(trimmed);
  if (modal) return { kind: "modal", modal, lines, turn: "unknown" };

  const composer = findComposer(trimmed);
  const turn = readTurn(trimmed, composer);

  // Numbered options with no composer and no modal we know: something is asking
  // something. Refuse, and let the user answer it in the raw pty.
  if (!composer && trimmed.some((line) => OPTION.test(line))) {
    return { kind: "unrecognised", reason: "unknown-dialog", lines, turn };
  }
  if (!composer)
    return { kind: "unrecognised", reason: "no-composer", lines, turn };
  return { kind: turn === "in-progress" ? "busy" : "idle", lines, turn };
}

/** Whether `text` may be written into the pty now, and the bytes to write. */
export function planInjection(
  screen: PromptScreen,
  text: string,
): InjectionPlan {
  if (!text.trim()) return { allowed: false, reason: "empty" };
  if (/^[/!#]/u.test(text.trimStart())) {
    return { allowed: false, reason: "command-prefix" };
  }
  switch (screen.kind) {
    case "permission-prompt":
      return { allowed: false, reason: "permission-prompt" };
    case "modal":
      return { allowed: false, reason: "modal" };
    case "unrecognised":
      return { allowed: false, reason: "unrecognised-screen" };
    case "idle":
    case "busy":
      return {
        allowed: true,
        bytes: `\x1b[200~${text}\x1b[201~\r`,
        queued: screen.kind === "busy",
      };
  }
}

function readPermissionPrompt(
  lines: readonly string[],
  trimmed: readonly string[],
): PermissionPrompt | undefined {
  const at = lastIndexMatching(trimmed, QUESTION_START);
  if (at < 0) return undefined;

  // The CLI hard-wraps box text into painted lines of its own, so a question
  // longer than the terminal is wide arrives in pieces. Join them back until
  // one ends the sentence.
  let question = trimmed[at];
  let asked = at;
  while (!question.endsWith("?") && asked + 1 < trimmed.length) {
    const next = trimmed[asked + 1];
    if (!next || OPTION.test(next)) break;
    question = `${question} ${next}`;
    asked += 1;
  }
  if (!QUESTION.test(question)) return undefined;

  const options: PromptOption[] = [];
  let footer = "";
  for (let i = asked + 1; i < trimmed.length; i += 1) {
    const line = trimmed[i];
    const option = OPTION.exec(line);
    if (option) {
      options.push({
        number: Number(option[2]),
        label: option[3].trim(),
        keystroke: option[2],
        selected: !!option[1],
      });
      continue;
    }
    if (!line) continue;
    if (options.length > 0 && PROMPT_FOOTER.test(line)) footer = line;
    break;
  }

  // Prefer refusing to a half-read prompt: the options must be the complete
  // run the TUI numbered, exactly one of them under the cursor, and the footer
  // the prompt's own.
  if (options.length < 2 || !footer) return undefined;
  if (options.some((option, index) => option.number !== index + 1))
    return undefined;
  if (options.filter((option) => option.selected).length !== 1)
    return undefined;

  return {
    question,
    detail: promptDetail(lines, trimmed, at),
    options,
    footer,
    ...(/\bEsc to cancel\b/u.test(footer)
      ? { cancel: { label: "Esc to cancel", keystroke: "\x1b" } }
      : {}),
  };
}

/** The prompt box above the question: the tool's header and what it would do. */
function promptDetail(
  lines: readonly string[],
  trimmed: readonly string[],
  question: number,
): string[] {
  const detail: string[] = [];
  for (let i = question - 1; i >= 0; i -= 1) {
    const line = trimmed[i];
    if (RULE.test(line)) break;
    if (!line) continue;
    if (/^[╌┄╍┅]{8,}$/u.test(line)) continue;
    detail.unshift(lines[i].trim());
  }
  return detail;
}

function readTrustModal(
  trimmed: readonly string[],
): RecognisedModal | undefined {
  const at = trimmed.findIndex((line) => TRUST_QUESTION.test(line));
  if (at < 0) return undefined;
  if (!trimmed.some((line) => TRUST_HEADER.test(line))) return undefined;
  if (!trimmed.some((line) => TRUST_FOOTER.test(line))) return undefined;
  const options = trimmed
    .slice(at)
    .map((line) => OPTION.exec(line))
    .filter((option): option is RegExpExecArray => !!option)
    .map((option) => `${option[2]}. ${option[3].trim()}`);
  return { modal: "trust-folder", title: trimmed[at], options };
}

type Composer = { top: number; bottom: number };

/**
 * The composer box: the bottom pair of rules with a `❯` line inside.
 *
 * `❯` also starts every user message in the scrollback, so the glyph alone is
 * not the composer. The box is, and a screen that has not repainted it has no
 * composer as far as this module is concerned.
 */
function findComposer(trimmed: readonly string[]): Composer | undefined {
  const bottom = lastIndexMatching(trimmed, RULE);
  if (bottom <= 0) return undefined;
  for (let top = bottom - 1; top >= 0; top -= 1) {
    if (!RULE.test(trimmed[top])) continue;
    const body = trimmed.slice(top + 1, bottom);
    return body.length > 0 && COMPOSER.test(body[0])
      ? { top, bottom }
      : undefined;
  }
  return undefined;
}

function readTurn(
  trimmed: readonly string[],
  composer: Composer | undefined,
): TurnState {
  const marker = lastContentLine(
    trimmed,
    composer ? composer.top : trimmed.length,
  );
  if (marker && SPINNER.test(marker)) return "in-progress";
  if (marker && COMPLETED.test(marker)) return "ended";
  // A composer with no spinner above it: an interrupt writes no record at all,
  // and this is the only thing that says the turn is over (§8).
  return composer ? "ended" : "unknown";
}

function lastContentLine(
  trimmed: readonly string[],
  before: number,
): string | undefined {
  for (let i = Math.min(before, trimmed.length) - 1; i >= 0; i -= 1) {
    if (trimmed[i]) return trimmed[i];
  }
  return undefined;
}

function lastIndexMatching(
  trimmed: readonly string[],
  pattern: RegExp,
): number {
  for (let i = trimmed.length - 1; i >= 0; i -= 1) {
    if (pattern.test(trimmed[i])) return i;
  }
  return -1;
}
