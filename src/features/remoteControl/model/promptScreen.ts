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
   * What to write into the pty to take this option: the option's own digit,
   * which selects and acts in one keystroke with no highlight to move first.
   * Measured over five runs against the transcript and the filesystem — `1`
   * returned a success `tool_result` and the file appeared, `3` returned
   * `is_error: true` with `"User rejected tool use"` and it did not. A digit no
   * option carries does nothing at all and leaves the prompt pending, which is
   * the benign way for this to fail.
   *
   * Nothing further is needed: a `\r` sent after the digit is mouse-mode and
   * cursor housekeeping the TUI answers with, not the actuator. On its own,
   * without a digit, `\r` takes whichever option the cursor sits on.
   */
  keystroke: string;
  /** The option under the TUI's `❯` cursor, which is the one a bare `\r` takes. */
  selected: boolean;
};

export type PermissionPrompt = {
  /** The question line, verbatim: `Do you want to create probe.txt?` */
  question: string;
  /** What the prompt box shows above the question: the tool header and its diff. */
  detail: readonly string[];
  options: readonly PromptOption[];
  /**
   * The prompt's own footer, verbatim: `Esc to cancel · Tab to amend`. Absent on
   * prompts that do not print one — a `WebFetch` domain prompt has no footer
   * anywhere on screen and puts its escape hint inside an option label instead.
   */
  footer?: string;
  /**
   * Present when the screen advertises Esc — in the footer, or in the label of
   * the option it stands for when there is no footer.
   *
   * It is called `deny` and not `cancel` because that is what it does: **Esc
   * denies the tool call, it does not dismiss the prompt unanswered.** Measured —
   * it produces the identical `tool_result` to option 3, `is_error: true` with
   * `"User rejected tool use"`, and the file is not written — and the WebFetch
   * prompt says so outright by putting `(esc)` in option 3's own label. `label`
   * quotes the screen, so it may well read "Esc to cancel"; the name describes
   * the effect. Do not offer it as a way out of deciding.
   *
   * Downstream consequence, since Esc and the No option are byte-identical in
   * the transcript: nothing reading the result can tell a cancel from an
   * explicit No, while MonoCode's `approval.resolved` separates `deny` from
   * `cancelled`. Only the client that sent the keystroke knows which happened,
   * so it has to remember.
   */
  deny?: { label: string; keystroke: string };
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
      /**
       * Whether the TUI will hold the message until the current turn ends rather
       * than sending it now. It queues cleanly — measured as
       * `queue-operation:enqueue` then a `user` record with
       * `promptSource:"queued"` — so nothing is lost either way; this only says
       * which of the two happened, and `"unknown"` says we cannot tell.
       *
       * Keyed on `turn`, not `kind`, which is the whole point: `kind` answers
       * "may I inject", `turn` answers "will it queue". Conflating them made this
       * report `false` on every mid-stream send, because a streaming screen is
       * `kind: "idle"` with `turn: "unknown"` — allowed, and queued, and reported
       * as neither. A caller must not render `"unknown"` as either one.
       */
      queued: boolean | "unknown";
    }
  | { allowed: false; reason: InjectionRefusal };

const RULE = /^[─━]{8,}$/u;
const OPTION = /^(❯\s*)?(\d+)\.\s+(\S.*)$/u;
const QUESTION_START = /^Do you want to /u;
const QUESTION = /^Do you want to .+\?$/u;
const PROMPT_FOOTER = /Esc to cancel/u;
/** How a footerless prompt advertises Esc: inside the option it stands for. */
const ESC_HINT = /\(esc\)/iu;
const COMPOSER = /^❯(?:\s|$)/u;
/**
 * A spinner frame: a glyph, then one *word* ending in an ellipsis — `✢ Osmosing…`.
 *
 * Requiring a word rather than any token is what keeps the welcome box out. Its
 * abbreviated project path is a non-alphanumeric glyph followed by something
 * containing an ellipsis (`│  /…/scratchpad/keytest  │`), so a looser pattern
 * calls it a spinner. Across sixteen captures the tightened form keeps all 27
 * real spinner lines and drops exactly those 3 path lines. It was never the
 * decisive marker there, but a false spinner reads as a live turn, and the next
 * screen where it lands above the composer would be one.
 */
const SPINNER = /^[^\p{L}\p{N}\s]\s+\p{L}[\p{L}\p{M}'’-]*…/u;
/**
 * The line a finished turn leaves behind: `✻ Baked for 2s`, and past a minute
 * `✻ Brewed for 4m 12s` — the duration switches format at 60 seconds, which an
 * earlier seconds-only version of this missed. The hour group is precaution
 * rather than observation: the longest turn measured was 4m 12s.
 *
 * The leading glyph and the single word before `for` are what keep prose out. An
 * assistant sentence ending "…waited for 3s" is a finished-turn summary to a
 * matcher that only looks at the tail, and it would be the last line above the
 * composer while the turn was still streaming. Matching the whole shape costs
 * nothing: across fifteen captures it accepts every real summary — twelve
 * distinct words so far — and rejects nothing it used to accept. A two-word
 * status would fall outside it and read as `unknown`, which is the safe way to
 * be wrong.
 *
 * The `$` stays. A running turn's counter carries a nested duration in raw
 * seconds (`✻ Osmosing… (1m 25s · thought for 83s)`) which an unanchored match
 * would read as a finished turn — the dangerous direction. `readTurn` tests the
 * spinner first, so the anchor is the second guard rather than the only one.
 *
 * The anchor also couples this to its caller in a way the pattern does not show:
 * `✻ Baked for 2s ` with the trailing space the TUI actually paints does not
 * match. It only ever sees a trimmed line because `readRenderedScreen` trims
 * before testing. Move the trimming and this stops matching anything, silently.
 */
const COMPLETED =
  /^[^\p{L}\p{N}\s]\s+\S+ for (?:\d+h )?(?:\d+m )?\d+(?:\.\d+)?s$/u;
const TRUST_QUESTION = /Is this a project you created or one you trust\?/u;
const TRUST_HEADER = /^Accessing workspace:$/u;
const TRUST_FOOTER = /Enter to confirm/u;

const CSI = /\x1b\[([0-9;:<=>?]*)[ -/]*([@-~])/y;
const OSC = /\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)/y;
const CHARSET = /\x1b[()*+][0-9A-Za-z]/y;
/** An escape cut in half by the end of the chunk; the rest is in the next one. */
const PARTIAL = /^\x1b(?:\[[0-9;:<=>?]*[ -/]*|\][^\x07\x1b]*|[()*+]?)?$/;

/**
 * A screen the TUI paints into, one chunk at a time.
 *
 * A quarter of a terminal emulator: enough of it that the text lands where the
 * TUI put it. The CLI paints words at absolute columns rather than writing
 * spaces, so stripping escapes instead of placing cells runs `Do you want to`
 * together into `Doyouwantto` and loses every column the layout depends on.
 *
 * Stateful because a terminal is, and that is the whole point: the grid has to
 * outlive the chunks. Re-rendering a retained buffer from blank looks
 * equivalent and is not. A live caller's buffer is bounded — `trimReplay` drops
 * the oldest chunks past 256KB — and every row painted before the retained
 * window and not repainted since comes back blank. The composer's two `─` rules
 * are exactly those rows: the `❯` line is repainted constantly, the box around
 * it is not. Measured on a 996KB capture: the whole stream reads `idle`, its
 * last 256KB reads `unrecognised`/`no-composer`, and a screen with no composer
 * refuses every send and cannot see an approval either. So feed each chunk in
 * once, and never re-feed.
 */
export type ScreenBuffer = {
  /** Apply one chunk of pty output. */
  write(chunk: string): void;
  /** The rows as text, trailing blanks trimmed — what the parser reads. */
  lines(): string[];
  /** What is on screen now. */
  screen(): PromptScreen;
};

/** A screen of `size`, blank, waiting to be written into. */
export function createScreenBuffer(size: ScreenSize): ScreenBuffer {
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
  /**
   * An escape cut in half by the end of a chunk, held for the next one.
   *
   * A stateless render could drop it and lose nothing, because the following
   * call re-rendered every byte anyway. Here each chunk is seen once, so a
   * discarded half would paint its own tail as text: `\x1b[3` followed by
   * `1mX` would put `1mX` on the screen.
   */
  let pending = "";

  const write = (chunk: string) => {
    const output = pending + chunk;
    pending = "";
    let i = 0;
    while (i < output.length) {
      const ch = output[i];
      if (ch === "\x1b") {
        if (PARTIAL.test(output.slice(i))) {
          pending = output.slice(i);
          break;
        }
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
    // An escape that never completes would otherwise hold every byte after it.
    // A title is the longest legitimate one by far, so past this the stream is
    // malformed and the half is worth less than the memory.
    if (pending.length > 4096) pending = "";
  };

  const lines = () => grid.map((line) => line.join("").replace(/\s+$/u, ""));

  return { write, lines, screen: () => readRenderedScreen(lines()) };
}

/**
 * Render a whole stream in one go.
 *
 * Starts from a blank grid, so it may only be given every byte the TUI ever
 * wrote. For a live pty, hold a `createScreenBuffer` instead.
 */
export function renderScreen(output: string, size: ScreenSize): string[] {
  const buffer = createScreenBuffer(size);
  buffer.write(output);
  return buffer.lines();
}

/**
 * Read the screen the TUI has painted in `output`.
 *
 * This is the only answer to "is a prompt pending". The transcript mirror's
 * `MirrorState.tools` looks like it could answer too and cannot: entries are
 * only ever set and read, never deleted, so a resolved call stays in the map
 * and "still in `tools`" is true forever. Whoever shows a pending approval has
 * to track that state themselves, from these results.
 */
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
        queued:
          screen.turn === "ended"
            ? false
            : screen.turn === "in-progress"
              ? true
              : "unknown",
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

  // Prefer refusing to a half-read prompt: the options must be the complete run
  // the TUI numbered, with exactly one of them under the cursor.
  //
  // The footer used to be required too, on the reasoning that it proved the run
  // was complete. It does not prove much that the numbering does not, and a real
  // `WebFetch` domain prompt has no footer at all — which made a whole class of
  // approvals permanently unanswerable. What is genuinely given up: a run painted
  // halfway is contiguous by construction, so `1..2 of 3` now parses as a
  // complete pair. Nothing on the screen distinguishes those, and the caller
  // re-reads on every chunk, so the next frame corrects it.
  if (options.length < 2) return undefined;
  if (options.some((option, index) => option.number !== index + 1))
    return undefined;
  if (options.filter((option) => option.selected).length !== 1)
    return undefined;

  // Esc is advertised either by the footer or, when there is no footer, by a
  // hint inside the option it corresponds to — `3. No, and tell Claude what to
  // do differently (esc)`. Either way the key denies, so either way it is `deny`.
  const escOption = options.find((option) => ESC_HINT.test(option.label));
  const deny =
    footer && /\bEsc to cancel\b/u.test(footer)
      ? { label: "Esc to cancel", keystroke: "\x1b" }
      : escOption
        ? { label: escOption.label, keystroke: "\x1b" }
        : undefined;

  return {
    question,
    detail: promptDetail(lines, trimmed, at),
    options,
    ...(footer ? { footer } : {}),
    ...(deny ? { deny } : {}),
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

/**
 * What the composer box is holding, or `null` when it is empty or absent.
 *
 * Exported because the glyph alone is not enough and the caller kept getting
 * that wrong. `❯` starts every user message in the scrollback too, so a scan
 * for the first `❯` row returns the oldest message on screen and calls it
 * unsent text. The consequences were both silent: a send cleared the composer,
 * looked again, found that scrollback row unchanged, and refused — so composer
 * sends failed for as long as any earlier message was visible — and the same
 * reading told `turnSignal` a composer was held, which ends a running turn
 * after three seconds of quiet.
 *
 * Only the first row of the box is read. A wrapped message fills more, but
 * "holding something" is answered by the first one.
 */
export function composerContent(lines: readonly string[]): string | null {
  const trimmed = lines.map((line) => line.trim());
  const composer = findComposer(trimmed);
  if (!composer) return null;
  const held = trimmed[composer.top + 1].slice(1).trim();
  return held.length > 0 ? held : null;
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
  // Nothing above the composer says anything about the turn, so neither does
  // this. An earlier version answered "ended" here, reasoning that a composer
  // with no spinner over it had to mean idle. It does not: once the assistant
  // starts streaming text the spinner leaves the visible grid entirely and the
  // line above the composer is prose, which looks exactly like this. Measured
  // over one 252-second turn replayed in 4KB frames, that guess was wrong for
  // 337 consecutive frames — about 166 seconds of a turn that was still
  // running.
  //
  // The cost is that an interrupted turn is "unknown" too, since the screen
  // carries no positive trace of one (§8), and resolving that is not something a
  // frame can do. Not by an empty composer either, which an earlier version of
  // this comment suggested: the CLI restores the interrupted prompt *into* the
  // composer, so on a real interrupt the composer holds text. A composer holding
  // text with no spinner above it is the trace — and it is also what a user
  // typing ahead mid-turn looks like, which the TUI allows and queues. Telling
  // those two apart needs the transcript: `turn_duration` where there is one,
  // and otherwise whether a user record opened a turn that nothing has closed.
  return "unknown";
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
