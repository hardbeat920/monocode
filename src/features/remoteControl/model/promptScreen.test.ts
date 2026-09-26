import { describe, expect, it } from "vitest";
import {
  createScreenBuffer,
  planInjection,
  readPromptScreen,
  readRenderedScreen,
  renderScreen,
  type PromptScreen,
} from "./promptScreen";

/**
 * Every fixture here is bytes or a screen from a real interactive session,
 * captured in `scratchpad/pty_raw*.bin` while probing remote control. The two
 * byte fixtures are verbatim slices; the screen fixtures are the bottom rows of
 * what `renderScreen` makes of a named slice, and each was checked to classify
 * the same trimmed as it does whole. Nothing here was written from imagination:
 * a fixture we invented would only prove the parser agrees with us.
 */

/** pty_raw4.bin[9483:10489], the frame that painted a real Write permission prompt. */
const PERMISSION_PROMPT_BYTES =
  "\x1b[H\r\x1b[30B\x1b[38;2;177;185;249m──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────\r\x1b[1C\x1b[1B\x1b[1mCreate file\r\x1b[1C\x1b[1B\x1b[22m\x1b[38;2;153;153;153mprobe.txt\r\x1b[1B\x1b[38;2;80;80;80m╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌\r\x1b[1C\x1b[1B\x1b[38;2;248;248;242m\x1b[2m 1 \x1b[22mhello\r\x1b[1B\x1b[38;2;80;80;80m╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌╌\r\x1b[1C\x1b[1B\x1b[39mDo\x1b[5Gyou\x1b[9Gwant\x1b[14Gto\x1b[17Gcreate\x1b[24G\x1b[1mprobe.txt\x1b[22m?\r\x1b[1B \x1b[38;2;177;185;249m❯\x1b[39m \x1b[38;2;153;153;153m1. \x1b[38;2;177;185;249mYes\x1b[39m\x1b[K\r\x1b[1B   \x1b[38;2;153;153;153m2. \x1b[39mYes, allow all edits during this session \x1b[1m(shift+tab)\x1b[22m\x1b[K\r\x1b[3C\x1b[1B\x1b[38;2;153;153;153m3. \x1b[39mNo\r\x1b[1B\x1b[K\r\x1b[1B \x1b[38;2;153;153;153mEsc to cancel · Tab to amend\r\x1b[1B\x1b[39m\x1b[K\r\x1b[2C\x1b[1B\x1b[K\r\x1b[2C\x1b[1B\x1b[K\x1b[45;1H\x1b[38;2H\x1b(B\u000f\x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b";
const PERMISSION_PROMPT_SIZE = { cols: 130, rows: 45 };

/** pty_raw.bin[0:1186], the first-run trust dialog, still pending. */
const TRUST_DIALOG_BYTES =
  "\x1b7\x1b[r\x1b8\x1b[?25h\x1b[?25l\x1b[?2004h\x1b[?1004h\x1b[?2031h\r\r\n\x1b[38;2;255;193;7m────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────\x1b[39m\r\r\n\x1b[2G\x1b[38;2;255;193;7m\x1b[1mAccessing\x1b[12Gworkspace:\x1b[22m\x1b[39m\r\r\n\r\r\n\x1b[2G\x1b[1m/private/tmp/claude-501/-Users-medeni-Desktop-Projects-monocode/bfba8763-531d-4557-ae2d-e7004d663f72/scratchpad/rctest\x1b[22m\r\r\n\r\r\n\x1b[2GQuick\x1b[8Gsafety\x1b[15Gcheck:\x1b[22GIs\x1b[25Gthis\x1b[30Ga\x1b[32Gproject\x1b[40Gyou\x1b[44Gcreated\x1b[52Gor\x1b[55Gone\x1b[59Gyou\x1b[63Gtrust?\x1b[70G(Like\x1b[76Gyour\x1b[81Gown\x1b[85Gcode,\x1b[91Ga\x1b[93Gwell-known\x1b[104Gopen\x1b[109Gsource\r\r\n\x1b[2Gproject,\x1b[11Gor\x1b[14Gwork\x1b[19Gfrom\x1b[24Gyour\x1b[29Gteam).\x1b[36GIf\x1b[39Gnot,\x1b[44Gtake\x1b[49Ga\x1b[51Gmoment\x1b[58Gto\x1b[61Greview\x1b[68Gwhat's\x1b[75Gin\x1b[78Gthis\x1b[83Gfolder\x1b[90Gfirst.\r\r\n\r\r\n\x1b[2GClaude\x1b[9GCode'll\x1b[17Gbe\x1b[20Gable\x1b[25Gto\x1b[28Gread,\x1b[34Gedit,\x1b[40Gand\x1b[44Gexecute\x1b[52Gfiles\x1b[58Ghere.\r\r\n\r\r\n\x1b[2G\x1b[38;2;153;153;153mSecurity\x1b[11Gguide\x1b[39m\r\r\n\r\r\n\x1b[2G\x1b[38;2;177;185;249m❯\x1b[4G\x1b[38;2;153;153;153m1.\x1b[7G\x1b[38;2;177;185;249mYes,\x1b[12GI\x1b[14Gtrust\x1b[20Gthis\x1b[25Gfolder\x1b[39m\r\r\n\x1b[4G\x1b[38;2;153;153;153m2.\x1b[7G\x1b[39mNo,\x1b[11Gexit\r\r\n\r\r\n\x1b[2G\x1b[38;2;153;153;153mEnter\x1b[8Gto\x1b[11Gconfirm\x1b[19G·\x1b[21GEsc\x1b[25Gto\x1b[28Gcancel\x1b[39m\r\r\n";
const TRUST_DIALOG_SIZE = { cols: 120, rows: 40 };

/** pty_raw.bin[0:5816] rendered at 120x40: a composer with nothing running. */
const IDLE_COMPOSER: string[] = [
  "────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────",
  "❯",
  "────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────",
  "  ⬆ /gsd:update │ Opus 5 (1M context) │ rctest                                                                      /rc",
  "  ⏸ plan mode on (shift+tab to cycle) · ← for agents",
];

/** pty_raw6.bin[0:10058] rendered at 130x45: mid-turn, spinner above the composer. */
const TURN_RUNNING: string[] = [
  "",
  "✢ Synthesizing…",
  "",
  "──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────",
  "❯",
  "──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────",
  "  ⬆ /gsd:update │ Opus 5 │ rctest ░░░░░░░░░░ 5%                                                                               /rc",
  "  ⏸ manual mode on · ←",
];

/**
 * pty_raw6.bin[0:22048] rendered at 130x45: the screen 25s after ESC interrupted
 * the turn. The transcript recorded nothing at all for this (§8), so the
 * composer coming back is the only evidence the turn is over.
 */
const AFTER_INTERRUPT: string[] = [
  "",
  "  /remote-control is active · Continue here, on your phone, or at https://claude.ai/code/session_01P9pJfVXWNcrT2zAP3NHShE",
  "",
  "──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────",
  "❯ Write a very long essay about the history of the abacus, at least 2000 words. Think carefully first.",
  "──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────",
  "  ⬆ /gsd:update │ Opus 5 │ rctest ░░░░░░░░░░ 5%                                                                               /rc",
  "  ⏸ manual mode on",
];

/**
 * interrupt_raw.bin[0:819200] rendered at 130x45: frame 200 of one 252-second
 * turn, mid-stream. The assistant is writing; the spinner has scrolled off the
 * grid entirely, so the line above the composer is prose. This screen is why
 * "a composer with no spinner is idle" is not a rule — it was wrong for 337
 * consecutive frames of this capture.
 */
const STREAMING_MID_TURN: string[] = [
  "  counters were pushed. The bench of the money-changer, banca, gave us bank, and a broken bench, banca rotta, gave us bankruptcy.",
  "",
  "",
  "──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────",
  "❯",
  "──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────",
  "  ⬆ /gsd:update │ Opus 5 (1M context) │ keytest ░░░░░░░░░░ 5%                                                                 /rc",
  "  ⏸ manual mode on · ← for agents",
];

/** pty_raw2.bin[0:200] rendered at 130x45: the banner, before the TUI is up. */
const BANNER_ONLY: string[] = ["", "╭─── Claude Code v2.1.221"];

/**
 * shape_fetch.bin rendered at 130x45, rows 22-32: a real `WebFetch` domain
 * prompt. It prints no footer — `Esc to cancel` appears nowhere on the screen —
 * and advertises Esc inside option 3 instead.
 */
const WEBFETCH_PROMPT: string[] = [
  "──────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────",
  " Fetch",
  "",
  '   url: "https://raw.githubusercontent.com/anthropics/anthropic-sdk-typescript/main/packages/sdk/README.md", prompt: "What is',
  '   the very first heading in this document? Quote it exactly, including its markdown level."',
  "   Claude wants to fetch content from raw.githubusercontent.com",
  "",
  " Do you want to allow Claude to fetch this content?",
  " ❯ 1. Yes",
  "   2. Yes, and don't ask again for raw.githubusercontent.com",
  "   3. No, and tell Claude what to do differently (esc)",
];

function promptScreen(): PromptScreen {
  return readPromptScreen(PERMISSION_PROMPT_BYTES, PERMISSION_PROMPT_SIZE);
}

describe("rendering what the TUI painted", () => {
  it("places words the CLI wrote at absolute columns", () => {
    // The bytes never contain the question as text: the CLI paints each word at
    // a column of its own, so stripping escapes would run them together.
    expect(PERMISSION_PROMPT_BYTES).not.toContain("Do you want");
    expect(PERMISSION_PROMPT_BYTES).toContain("\x1b[17Gcreate");

    const lines = renderScreen(PERMISSION_PROMPT_BYTES, PERMISSION_PROMPT_SIZE);
    expect(lines.map((line) => line.trim())).toContain(
      "Do you want to create probe.txt?",
    );
    expect(lines.map((line) => line.trim())).toContain(
      "2. Yes, allow all edits during this session (shift+tab)",
    );
  });

  it("drops an escape the capture cut in half instead of printing it", () => {
    // This capture really does end mid-sequence, which is what a chunk boundary
    // looks like to a subscriber.
    expect(PERMISSION_PROMPT_BYTES.endsWith("\x1b")).toBe(true);
    const lines = renderScreen(PERMISSION_PROMPT_BYTES, PERMISSION_PROMPT_SIZE);
    expect(lines.join("\n")).not.toContain("[?1000h");
  });
});

describe("a permission prompt on screen", () => {
  it("is read into its question, detail and numbered options", () => {
    const screen = promptScreen();
    expect(screen.kind).toBe("permission-prompt");
    if (screen.kind !== "permission-prompt") return;

    expect(screen.prompt.question).toBe("Do you want to create probe.txt?");
    expect(screen.prompt.detail).toEqual([
      "Create file",
      "probe.txt",
      "1 hello",
    ]);
    expect(screen.prompt.options).toEqual([
      { number: 1, label: "Yes", keystroke: "1", selected: true },
      {
        number: 2,
        label: "Yes, allow all edits during this session (shift+tab)",
        keystroke: "2",
        selected: false,
      },
      { number: 3, label: "No", keystroke: "3", selected: false },
    ]);
    expect(screen.prompt.footer).toBe("Esc to cancel · Tab to amend");
    expect(screen.prompt.deny).toEqual({
      label: "Esc to cancel",
      keystroke: "\x1b",
    });
  });

  it("means the turn is still running and no message may be injected", () => {
    const screen = promptScreen();
    expect(screen.turn).toBe("in-progress");
    expect(planInjection(screen, "carry on")).toEqual({
      allowed: false,
      reason: "permission-prompt",
    });
  });

  it("is refused when the option run is broken or nothing is selected", () => {
    const lines = renderScreen(PERMISSION_PROMPT_BYTES, PERMISSION_PROMPT_SIZE);
    // The same real screen with option 2 renumbered, and with the cursor taken
    // off every option. These are the checks that carry the weight now that a
    // missing footer no longer disqualifies a prompt.
    const renumbered = lines.map((line) =>
      line.includes("2. Yes, allow all") ? line.replace("2.", "4.") : line,
    );
    expect(readRenderedScreen(renumbered).kind).toBe("unrecognised");
    const uncursored = lines.map((line) => line.replace("❯ 1.", "  1."));
    expect(readRenderedScreen(uncursored).kind).toBe("unrecognised");
    expect(JSON.stringify(readRenderedScreen(uncursored))).not.toContain(
      "keystroke",
    );
  });

  it("reads a prompt that prints no footer at all", () => {
    // Verbatim rows 22-32 of shape_fetch.bin rendered at 130x45: a real WebFetch
    // domain prompt. There is no `Esc to cancel` anywhere on that screen — the
    // escape hint lives inside option 3 instead — and requiring a footer made
    // every domain approval permanently unanswerable.
    const screen = readRenderedScreen(WEBFETCH_PROMPT);
    expect(screen.kind).toBe("permission-prompt");
    if (screen.kind !== "permission-prompt") return;

    expect(screen.prompt.question).toBe(
      "Do you want to allow Claude to fetch this content?",
    );
    expect(screen.prompt.footer).toBeUndefined();
    expect(screen.prompt.options.map((option) => option.keystroke)).toEqual([
      "1",
      "2",
      "3",
    ]);
    expect(screen.prompt.detail).toContain(
      "Claude wants to fetch content from raw.githubusercontent.com",
    );
    // Esc is option 3 here, by the TUI's own label, which is the same denial the
    // footer form advertises.
    expect(screen.prompt.deny).toEqual({
      label: "No, and tell Claude what to do differently (esc)",
      keystroke: "\x1b",
    });
    expect(planInjection(screen, "carry on")).toEqual({
      allowed: false,
      reason: "permission-prompt",
    });
  });

  it("reads a question the box wrapped onto a second line", () => {
    // The pty runs at a fixed 120 columns, narrower than the 130 this was
    // captured at, and the CLI hard-wraps box text into painted lines of its
    // own — the trust dialog's paragraph arrives that way. The real question
    // re-wrapped is the closest thing the captures give us to a long path.
    const lines = renderScreen(
      PERMISSION_PROMPT_BYTES,
      PERMISSION_PROMPT_SIZE,
    ).flatMap((line) =>
      line.includes("Do you want to create")
        ? [" Do you want to create", " probe.txt?"]
        : [line],
    );
    const screen = readRenderedScreen(lines);
    expect(screen.kind).toBe("permission-prompt");
    if (screen.kind !== "permission-prompt") return;
    expect(screen.prompt.question).toBe("Do you want to create probe.txt?");
    expect(screen.prompt.options).toHaveLength(3);
  });
});

describe("a modal on screen", () => {
  it("blocks injection and is never answered for the user", () => {
    const screen = readPromptScreen(TRUST_DIALOG_BYTES, TRUST_DIALOG_SIZE);
    expect(screen.kind).toBe("modal");
    if (screen.kind !== "modal") return;

    expect(screen.modal.modal).toBe("trust-folder");
    expect(screen.modal.title).toContain(
      "Is this a project you created or one you trust?",
    );
    expect(screen.modal.options).toEqual([
      "1. Yes, I trust this folder",
      "2. No, exit",
    ]);
    // The whole point of the first probe's failure: the message and its return
    // went into this dialog instead of the composer.
    expect(planInjection(screen, "hello from MonoCode")).toEqual({
      allowed: false,
      reason: "modal",
    });
    expect(JSON.stringify(screen)).not.toContain("keystroke");
  });
});

describe("an idle composer", () => {
  it("accepts a message as bracketed paste", () => {
    const screen = readRenderedScreen(IDLE_COMPOSER);
    expect(screen.kind).toBe("idle");
    // Nothing above the composer, so nothing is claimed about the turn.
    expect(screen.turn).toBe("unknown");
    expect(planInjection(screen, "say ALPHA")).toEqual({
      allowed: true,
      bytes: "\x1b[200~say ALPHA\x1b[201~\r",
      // Nothing above this composer, so the screen cannot say whether a turn is
      // running — and it will not pretend it can.
      queued: "unknown",
    });
  });

  it("keeps multi-line text in one message", () => {
    const screen = readRenderedScreen(IDLE_COMPOSER);
    const plan = planInjection(screen, "line one\nline two");
    expect(plan).toEqual({
      allowed: true,
      bytes: "\x1b[200~line one\nline two\x1b[201~\r",
      queued: "unknown",
    });
  });

  it("refuses text the TUI would take as one of its own commands", () => {
    const screen = readRenderedScreen(IDLE_COMPOSER);
    for (const text of ["/compact", "!ls", "#remember this"]) {
      expect(planInjection(screen, text)).toEqual({
        allowed: false,
        reason: "command-prefix",
      });
    }
    expect(planInjection(screen, "   ")).toEqual({
      allowed: false,
      reason: "empty",
    });
  });
});

describe("turn state", () => {
  it("reads a spinner above the composer as a turn in progress", () => {
    const screen = readRenderedScreen(TURN_RUNNING);
    expect(screen.turn).toBe("in-progress");
    expect(screen.kind).toBe("busy");
    // Measured: a message sent mid-turn is queued and arrives when the turn
    // ends, so it is still worth sending.
    expect(planInjection(screen, "and then say BETA")).toEqual({
      allowed: true,
      bytes: "\x1b[200~and then say BETA\x1b[201~\r",
      queued: true,
    });
  });

  it("does not claim an interrupted turn ended, because the screen cannot say", () => {
    // This was asserted as "ended" on the reasoning that a returned composer
    // ends a turn. A streaming turn produces the same screen, so the rule was
    // unsound; the honest answer is that an interrupt leaves no trace here, and
    // the caller has to resolve it from the transcript.
    const screen = readRenderedScreen(AFTER_INTERRUPT);
    expect(screen.turn).toBe("unknown");
    expect(screen.kind).toBe("idle");
  });

  it("does not take prose that happens to end in a duration as a summary", () => {
    // Constructed, not captured: no assistant line in the corpus ends this way,
    // which is why it stayed latent. It is the shape that matters — this line
    // would be the last one above the composer mid-stream.
    const screen = readRenderedScreen([
      "⏺ I ran the benchmark and waited for 3s",
      "",
      "──────────────────────────────────────────────",
      "❯",
      "──────────────────────────────────────────────",
      "  ⏸ manual mode on",
    ]);
    expect(screen.turn).toBe("unknown");
  });

  it("does not read the welcome box's shortened path as a spinner", () => {
    // The path line is verbatim from the corpus: the box centres
    // `/…/scratchpad/keytest`, and an ellipsis after a glyph is all a loose
    // spinner pattern needs. Put it where the marker is read from — directly
    // above the composer — and a loose pattern reports a live turn on a screen
    // where nothing is running. It has never landed there in a capture; this
    // pins the behaviour for when it does.
    const screen = readRenderedScreen([
      "│               /…/scratchpad/keytest                │",
      "──────────────────────────────────────────────",
      "❯",
      "──────────────────────────────────────────────",
      "  ⏸ manual mode on",
    ]);
    expect(screen.turn).not.toBe("in-progress");
    expect(screen.turn).toBe("unknown");
  });

  it("still reads a summary the TUI padded with trailing spaces", () => {
    // The `$` anchor only works because the screen is trimmed before matching.
    // This pins that contract: the padding the TUI actually paints must not stop
    // a finished turn from being recognised.
    expect(readRenderedScreen(["✻ Baked for 2s      "]).turn).toBe("ended");
  });

  it("does not report a streaming turn as ended", () => {
    const screen = readRenderedScreen(STREAMING_MID_TURN);
    expect(screen.turn).not.toBe("ended");
    expect(screen.turn).toBe("unknown");
  });

  it("reads a turn that ran past a minute as ended", () => {
    // `✻ Brewed for 4m 12s` is measured; the duration changes format at 60
    // seconds, and a long turn is exactly when someone leaves for their phone.
    // Without the composer box the summary line is the only evidence there is,
    // so this is the screen that catches a seconds-only matcher.
    const screen = readRenderedScreen([
      "⏺ Wrote the essay.",
      "",
      "✻ Brewed for 4m 12s",
    ]);
    expect(screen.turn).toBe("ended");
  });

  it("does not read a running turn's own counter as a finished one", () => {
    // Both measured. The elapsed time switches to `1m 25s`, while the nested
    // `thought for 83s` stays in raw seconds — which is what a matcher without
    // its end anchor would swallow.
    for (const counter of [
      "✻ Osmosing… (1m 25s · thought for 83s)",
      "✻ Ruminating… (4m 12s · ↓ 12.6k tokens)",
    ]) {
      expect(readRenderedScreen(["⏺ thinking", "", counter]).turn).toBe(
        "in-progress",
      );
    }
  });
});

describe("whether an injected message will be queued", () => {
  it("says so only when the screen says so", () => {
    // A spinner is up: it will queue, measured as enqueue then a queued user
    // record.
    const running = planInjection(readRenderedScreen(TURN_RUNNING), "go");
    expect(running).toMatchObject({ allowed: true, queued: true });

    // A finished turn's summary is on screen: it goes straight in.
    const finished = planInjection(
      readRenderedScreen([
        "⏺ done",
        "",
        "✻ Brewed for 4m 12s",
        "",
        "──────────────────────────────────────────────",
        "❯",
        "──────────────────────────────────────────────",
        "  ⏸ manual mode on",
      ]),
      "go",
    );
    expect(finished).toMatchObject({ allowed: true, queued: false });

    // Streaming: injection is allowed and the TUI will queue it, but the screen
    // carries no marker either way. This is the case that used to report false.
    const streaming = planInjection(
      readRenderedScreen(STREAMING_MID_TURN),
      "go",
    );
    expect(streaming).toMatchObject({ allowed: true, queued: "unknown" });
  });
});

describe("a screen we do not recognise", () => {
  it("is never guessed at", () => {
    const screen = readRenderedScreen(BANNER_ONLY);
    expect(screen).toMatchObject({
      kind: "unrecognised",
      reason: "no-composer",
    });
    expect(planInjection(screen, "hello")).toEqual({
      allowed: false,
      reason: "unrecognised-screen",
    });
  });

  it("stays unrecognised when an unfamiliar dialog is asking something", () => {
    const lines = renderScreen(PERMISSION_PROMPT_BYTES, PERMISSION_PROMPT_SIZE);
    // The real prompt screen with its question replaced: the options are still
    // there, but nothing says what this dialog is. No artifact captured an
    // AskUserQuestion or a plan approval, so this stands in for them.
    const unfamiliar = lines.map((line) =>
      line.includes("Do you want to create")
        ? line.replace(
            "Do you want to create probe.txt?",
            "Which one should I use?",
          )
        : line,
    );
    const screen = readRenderedScreen(unfamiliar);
    expect(screen).toMatchObject({
      kind: "unrecognised",
      reason: "unknown-dialog",
    });
    expect(planInjection(screen, "1")).toEqual({
      allowed: false,
      reason: "unrecognised-screen",
    });
  });

  it("offers no keystroke on any screen that is not a parsed prompt", () => {
    const screens = [
      readPromptScreen(TRUST_DIALOG_BYTES, TRUST_DIALOG_SIZE),
      readRenderedScreen(IDLE_COMPOSER),
      readRenderedScreen(TURN_RUNNING),
      readRenderedScreen(AFTER_INTERRUPT),
      readRenderedScreen(BANNER_ONLY),
      readRenderedScreen([]),
    ];
    for (const screen of screens) {
      expect(JSON.stringify(screen)).not.toContain("keystroke");
    }
  });
});

/**
 * pty_raw.bin[3200:4200] and [4200:4400], the two halves of one real idle
 * screen at 120x40: the first paints the composer's box, the second only
 * repaints inside and around it — the hand-over line, the session link, the
 * `/rc` indicator. Neither rule is touched by the second, which is the whole
 * point of the pair.
 */
const COMPOSER_PAINTED =
  '──────────────────────────────────────────────────────────────────────────────────────╯\r\x1b[103C\x1b[23B\x1b[38;2;153;153;153m● high · /effort\r\x1b[1B\x1b[38;2;136;136;136m────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────\r\x1b[1B\x1b[39m❯ \x1b[2mTry "how do I log an error?"\r\x1b[1B\x1b[22m\x1b[38;2;136;136;136m────────────────────────────────────────────────────────────────────────────────────────────────────────────────────────\r\x1b[104C\x1b[1B\x1b[38;2;255;193;7m/rc connecting…\r\x1b[2C\x1b[1B\x1b[38;2;72;150;140m⏸ plan mode on\x1b[38;2;153;153;153m (shift+tab to cycle) · ← for agents\x1b[39m\x1b[40;1H\x1b[37;3H\x1b[?25h\x1b[?25l\x1b[H\r\x1b[1C\x1b[13B\x1b[38;2;255;193;7m⚠\x1b[4G1 MCP server needs authentication\x1b[38;2;153;153;153m · run /mcp\r\x1b[2C\x1b[23B\x1b[39m\x1b[K\x1b[40;1H\x1b[37;3H\x1b[?25h\x1b[?25l\x1b[H\r\x1b[2C\x1b[38B\x1b[33m⬆ /gsd:update\x1b[38;2;153;153;153m │ \x1b[2mOpus 5 (1M context)\x1b[22m │ \x1b[2mrctest\x1b[22m\x1b[39m\x1b[40;1H\x1b[37;3H\x1b[?25h\x1b[?25l\x1b[H\r\x1b[2C\x1b[15B/remote-control\x1b[19Gis\x1b[22Gactive\x1b[38;2;153;153;153m · Continue here, on y';
const REPAINT_AROUND_IT =
  "our phone, or at \r\x1b[2C\x1b[1Bhttps://claude.ai/code/session_01LP8vc1MuWegwHowkjJNnRK\r\x1b[104C\x1b[22B\x1b[39m            \x1b[38;2;78;186;101m/rc\x1b[39m\x1b[40;1H\x1b[37;3H\x1b[?25h\x1b[?25l\x1b[H\r\x1b[103C\x1b[34B\x1b[K\x1b[40;1H\x1b[37;3H\x1b[?25h";

describe("a screen held across chunks", () => {
  it("keeps a row painted by a chunk it can no longer see", () => {
    const buffer = createScreenBuffer({ cols: 120, rows: 40 });
    buffer.write(COMPOSER_PAINTED);
    buffer.write(REPAINT_AROUND_IT);

    expect(buffer.screen().kind).toBe("idle");
  });

  it("loses that row when the same bytes are re-rendered from blank", () => {
    // What the caller's replay buffer gives you once `trimReplay` has dropped
    // the older chunks: the `❯` is still repainted, its box is not, and a
    // screen with no composer refuses every send. This is the bug the buffer
    // exists to stop, so it is asserted rather than described.
    const screen = readRenderedScreen(
      renderScreen(REPAINT_AROUND_IT, { cols: 120, rows: 40 }),
    );

    expect(screen).toMatchObject({
      kind: "unrecognised",
      reason: "no-composer",
    });
  });

  it("carries an escape cut in half by the end of a chunk", () => {
    const buffer = createScreenBuffer({ cols: 20, rows: 3 });
    buffer.write("A\x1b[3");
    buffer.write("1mB");

    // Not "A1mB": the half was held, not dropped, so its tail stayed an escape
    // rather than becoming text.
    expect(buffer.lines()[0]).toBe("AB");
  });
});
