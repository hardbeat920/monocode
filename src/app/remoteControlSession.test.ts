import { describe, expect, it } from "vitest";
import {
  composerHeld,
  noteInterruptedTurn,
  pendingAfter,
  remoteApprovalKeystroke,
  remoteApprovalReply,
  remoteApprovalView,
  remoteControlName,
  remoteControlStep,
  remoteControlTarget,
  remoteSendGate,
  seatRemoteUserMessage,
  shouldAutoOpen,
} from "./remoteControlSession";
import { newSession, type Session } from "../features/sessions/model/session";
import type {
  PermissionPrompt,
  PromptScreen,
} from "../features/remoteControl/model/promptScreen";

function session(patch: Partial<Session> = {}): Session {
  return { ...newSession("claude", "/repo/monocode"), ...patch };
}

describe("remote control naming", () => {
  it("names a session after its project", () => {
    expect(remoteControlName("/repo/monocode", [])).toBe("monocode");
  });

  it("distinguishes several threads of one project", () => {
    const first = remoteControlName("/repo/monocode", []);
    const second = remoteControlName("/repo/monocode", [first]);
    const third = remoteControlName("/repo/monocode", [first, second]);

    expect([first, second, third]).toEqual([
      "monocode",
      "monocode 2",
      "monocode 3",
    ]);
  });

  it("resolves against the names in use, not a count", () => {
    // The first thread closed. Its name is free, and the name already given to
    // the surviving process must not be handed out again either.
    expect(remoteControlName("/repo/monocode", ["monocode 2"])).toBe("monocode");
  });

  it("does not collide across projects", () => {
    expect(remoteControlName("/repo/other", ["monocode"])).toBe("other");
  });
});

describe("acting on a menu click", () => {
  it("opens and closes when the menu agreed with the state", () => {
    expect(remoteControlStep("open", false)).toBe("open");
    expect(remoteControlStep("close", true)).toBe("close");
  });

  it.each([
    ["open", true],
    ["close", false],
  ] as const)(
    "does nothing rather than flip when asked to %s against the state",
    (intent, open) => {
      // The click was aimed at something that has since changed. Doing the
      // other thing would carry out an instruction nobody gave.
      expect(remoteControlStep(intent, open)).toBe("none");
    },
  );
});

describe("remote control target", () => {
  it("reports the bound conversation and whether it is live", () => {
    const bound = session({ providerSessionId: "sess-abc" });

    expect(remoteControlTarget(bound, true)).toEqual({
      harness: "claude",
      providerSessionId: "sess-abc",
      active: true,
    });
  });
});

describe("an inbound message from the phone", () => {
  it("is seated as a user turn, not as a system note", () => {
    const next = seatRemoteUserMessage(session(), {
      type: "remote.userMessage",
      text: "  try the other branch  ",
    });

    const block = next.blocks.at(-1);
    // A user block, because that is what it is: a turn on this conversation
    // that did not come from this composer.
    expect(block?.role).toBe("user");
    expect(block?.text).toBe("try the other branch");
    expect(block?.startedAt).toBeTypeOf("number");
  });

  it("ignores an empty message rather than seating a blank turn", () => {
    const before = session();
    const next = seatRemoteUserMessage(before, {
      type: "remote.userMessage",
      text: "   ",
    });

    expect(next).toBe(before);
  });
});

describe("the cue for a turn the handover stopped", () => {
  it("explains the interruption when a turn was running", () => {
    const next = noteInterruptedTurn(session({ busy: true }));

    const block = next.blocks.at(-1);
    expect(block?.role).toBe("system");
    expect(block?.notice).toBe("error");
    expect(block?.text).toMatch(/stopped the turn that was running/);
  });

  it("says nothing when no turn was running", () => {
    const idle = session({ busy: false });

    expect(noteInterruptedTurn(idle)).toBe(idle);
  });
});

describe("all mode opening lazily", () => {
  const ready = session({ providerSessionId: "sess-abc", busy: false });

  it("opens a used, idle Claude thread", () => {
    expect(
      shouldAutoOpen(ready, { mode: "all", open: false, dismissed: false }),
    ).toBe(true);
  });

  it("does nothing in manual mode", () => {
    expect(
      shouldAutoOpen(ready, { mode: "manual", open: false, dismissed: false }),
    ).toBe(false);
  });

  it("waits for a conversation to exist", () => {
    // Before a first turn there is no bound id, so `--resume` would have
    // nothing to open. This is what makes the mode lazy rather than eager.
    expect(
      shouldAutoOpen(session({ providerSessionId: undefined }), {
        mode: "all",
        open: false,
        dismissed: false,
      }),
    ).toBe(false);
  });

  it("never interrupts a running turn", () => {
    // Opening stops the headless child, so auto-opening mid-turn would kill the
    // turn the user is waiting on without them asking for anything.
    expect(
      shouldAutoOpen({ ...ready, busy: true }, {
        mode: "all",
        open: false,
        dismissed: false,
      }),
    ).toBe(false);
  });

  it("does not reopen what the user closed", () => {
    expect(
      shouldAutoOpen(ready, { mode: "all", open: false, dismissed: true }),
    ).toBe(false);
  });

  it("does not open a second time", () => {
    expect(
      shouldAutoOpen(ready, { mode: "all", open: true, dismissed: false }),
    ).toBe(false);
  });

  it("leaves other harnesses alone", () => {
    expect(
      shouldAutoOpen(
        { ...newSession("codex", "/repo/monocode"), providerSessionId: "x" },
        { mode: "all", open: false, dismissed: false },
      ),
    ).toBe(false);
  });
});

const OPTIONS = [
  { number: 1, label: "Yes", keystroke: "1", selected: true },
  { number: 2, label: "Yes, allow all edits during this session", keystroke: "2", selected: false },
  { number: 3, label: "No", keystroke: "3", selected: false },
];

function promptScreen(patch: Partial<PermissionPrompt> = {}): PromptScreen {
  return {
    lines: ["Do you want to create probe.txt?"],
    turn: "in-progress",
    kind: "permission-prompt",
    prompt: {
      question: "Do you want to create probe.txt?",
      detail: ["Create file", "probe.txt", " 1 hello"],
      options: OPTIONS,
      footer: "Esc to cancel · Tab to amend",
      cancel: { label: "Esc to cancel", keystroke: "\x1b" },
      ...patch,
    },
  };
}

const idleScreen: PromptScreen = { lines: ["❯"], turn: "ended", kind: "idle" };

describe("what a remote session shows for a pending prompt", () => {
  it("shows the prompt when the transcript says something is waiting", () => {
    const view = remoteApprovalView({ pending: true, screen: promptScreen(), terminalOpen: false });

    expect(view.kind).toBe("question");
    if (view.kind !== "question") return;
    // Every option the TUI offered, not a yes/no reduction of them.
    expect(view.question.options.map((option) => option.label)).toEqual([
      "Yes",
      "Yes, allow all edits during this session",
      "No",
    ]);
    // The header and diff are the only statement of what the prompt would do.
    expect(view.question.header).toBe("Create file");
    expect(view.question.prompt).toContain("probe.txt");
    expect(view.question.prompt).toContain(" 1 hello");
    expect(view.cancel).toBe("Esc to cancel");
  });

  it("shows nothing when the transcript says nothing is waiting", () => {
    // A screen that looks like a prompt is not evidence that one is pending —
    // this is the composer-footer false positive the probe hit.
    expect(remoteApprovalView({ pending: false, screen: promptScreen(), terminalOpen: false })).toEqual({ kind: "none" });
  });

  it("stays quiet while a tool is merely running", () => {
    // An outstanding tool call is equally the ordinary state of a running tool,
    // so an idle or busy screen must not raise anything.
    expect(remoteApprovalView({ pending: true, screen: idleScreen, terminalOpen: false })).toEqual({ kind: "none" });
  });

  it.each([
    [
      "a dialog it cannot identify",
      { lines: ["1. Something", "2. Else"], turn: "in-progress", kind: "unrecognised", reason: "unknown-dialog" } as PromptScreen,
    ],
    [
      "a modal it must not answer",
      {
        lines: ["Is this a project you created or one you trust?"],
        turn: "unknown",
        kind: "modal",
        modal: { question: "Is this a project you created or one you trust?", detail: [], options: ["1. Yes"], footer: "Enter to confirm" },
      } as PromptScreen,
    ],
  ])("surfaces the raw screen for %s", (_label, screen) => {
    const view = remoteApprovalView({ pending: true, screen, terminalOpen: false });

    // Failing visible: something is waiting and cannot be read, which is exactly
    // when guessing is forbidden.
    expect(view.kind).toBe("raw");
    if (view.kind !== "raw") return;
    expect(view.lines).toBe(screen.lines);
  });

  it("does not raise a partial repaint as a question", () => {
    const view = remoteApprovalView({
      pending: true,
      terminalOpen: false,
      screen: {
        lines: ["some banner"],
        turn: "unknown",
        kind: "unrecognised",
        reason: "no-composer",
      },
    });

    // No numbered options means no evidence anything is asking.
    expect(view).toEqual({ kind: "none" });
  });
});

describe("answering a remote prompt", () => {
  it("sends the bare digit the TUI advertised", () => {
    expect(
      remoteApprovalKeystroke(promptScreen(), { kind: "option", id: "2" }),
    ).toBe("2");
  });

  it("does not append a carriage return", () => {
    // A bare digit selects and acts; a trailing CR is housekeeping and no second
    // action, so sending one would only invite a second interpretation.
    expect(
      remoteApprovalKeystroke(promptScreen(), { kind: "option", id: "1" }),
    ).not.toContain("\r");
  });

  it("refuses an option that is not on the screen", () => {
    expect(
      remoteApprovalKeystroke(promptScreen(), { kind: "option", id: "9" }),
    ).toBeNull();
  });

  it("refuses to answer a screen that is not a prompt", () => {
    // The whole point of the raw-pty fallback: an unreadable screen can never
    // produce an injected answer.
    expect(
      remoteApprovalKeystroke(idleScreen, { kind: "option", id: "1" }),
    ).toBeNull();
    expect(remoteApprovalKeystroke(idleScreen, { kind: "cancel" })).toBeNull();
  });

  it("sends Esc only when the prompt advertises it", () => {
    expect(remoteApprovalKeystroke(promptScreen(), { kind: "cancel" })).toBe(
      "\x1b",
    );
    expect(
      remoteApprovalKeystroke(promptScreen({ cancel: undefined }), {
        kind: "cancel",
      }),
    ).toBeNull();
  });

  it("records what was sent rather than what the result will say", () => {
    // Esc and option 3 produce byte-identical `tool_result` output, so this is
    // the only place the difference exists.
    expect(remoteApprovalReply({ kind: "option", id: "3" })).toEqual({
      kind: "answered",
      answers: { "remote-approval": ["3"] },
    });
    expect(remoteApprovalReply({ kind: "cancel" })).toEqual({
      kind: "skipped",
    });
  });
});

describe("tracking what the transcript says is outstanding", () => {
  it("opens on a tool call and closes on its result", () => {
    const started = pendingAfter(new Set(), [
      { type: "tool.started", callId: "t1", title: "Write", status: "pending" },
    ]);
    expect([...started]).toEqual(["t1"]);

    const done = pendingAfter(started, [
      { type: "tool.updated", callId: "t1", title: "Write", status: "completed" },
    ]);
    expect([...done]).toEqual([]);
  });

  it("stays open while the call is only progressing", () => {
    const pending = pendingAfter(new Set(["t1"]), [
      { type: "tool.updated", callId: "t1", title: "Write", status: "running" },
    ]);

    expect([...pending]).toEqual(["t1"]);
  });

  it("closes a call the CLI reported as failed", () => {
    // A rejected tool use arrives as an error result, which still ends the call.
    const pending = pendingAfter(new Set(["t1"]), [
      { type: "tool.updated", callId: "t1", title: "Write", status: "failed" },
    ]);

    expect([...pending]).toEqual([]);
  });
});

/** Rendered composer rows, copied from the parser's own capture fixtures. */
const IDLE_COMPOSER = ["─".repeat(20), "❯", "─".repeat(20), "  ⏸ plan mode on"];
const AFTER_INTERRUPT = [
  "─".repeat(20),
  "❯ Write a very long essay about the history of the abacus, at least 2000 words. Think carefully first.",
  "─".repeat(20),
  "  ⏸ manual mode on",
];

const allowed = { allowed: true, bytes: "\x1b[200~say OK\x1b[201~\r", queued: false } as const;

function idleWith(lines: readonly string[]): PromptScreen {
  return { lines, turn: "ended", kind: "idle" };
}

describe("the composer the CLI restores after an interrupt", () => {
  it("reads nothing out of an empty composer", () => {
    // Measured: an idle composer renders as a bare marker, with no placeholder
    // text to mistake for content.
    expect(composerHeld(IDLE_COMPOSER)).toBeNull();
  });

  it("reads the restored prompt out of a held composer", () => {
    expect(composerHeld(AFTER_INTERRUPT)).toBe(
      "Write a very long essay about the history of the abacus, at least 2000 words. Think carefully first.",
    );
  });

  it("finds no composer on a screen without one", () => {
    expect(composerHeld(["╭─── Claude Code v2.1.221"])).toBeNull();
  });

  it("sends when the composer is empty", () => {
    expect(remoteSendGate(idleWith(IDLE_COMPOSER), allowed)).toEqual({
      kind: "send",
    });
  });

  it("clears first when the composer is holding text", () => {
    // `planInjection` says yes here — idle screen, no modal — which is exactly
    // why this is a third outcome and not a refusal reason.
    const gate = remoteSendGate(idleWith(AFTER_INTERRUPT), allowed);

    expect(gate.kind).toBe("clear");
    if (gate.kind !== "clear") return;
    expect(gate.held).toContain("abacus");
  });

  it("refuses when the parser refused, whatever the composer holds", () => {
    const gate = remoteSendGate(idleWith(IDLE_COMPOSER), {
      allowed: false,
      reason: "modal",
    });

    expect(gate.kind).toBe("refuse");
    if (gate.kind !== "refuse") return;
    expect(gate.reason).toMatch(/dialog/);
  });

  it("refuses before the terminal has painted", () => {
    expect(remoteSendGate(null, null).kind).toBe("refuse");
  });
});

describe("while the user has the terminal open", () => {
  it("raises nothing, even on a prompt it could read", () => {
    // Two surfaces for one prompt invites two answers, and the second digit
    // lands on whatever screen the first one produced.
    expect(
      remoteApprovalView({
        pending: true,
        screen: promptScreen(),
        terminalOpen: true,
      }),
    ).toEqual({ kind: "none" });
  });

  it("does not surface the raw screen twice", () => {
    expect(
      remoteApprovalView({
        pending: true,
        terminalOpen: true,
        screen: {
          lines: ["1. Something"],
          turn: "in-progress",
          kind: "unrecognised",
          reason: "unknown-dialog",
        },
      }),
    ).toEqual({ kind: "none" });
  });

  it("refuses to inject over the user's own typing", () => {
    const gate = remoteSendGate(idleWith(IDLE_COMPOSER), allowed, true);

    expect(gate.kind).toBe("refuse");
    if (gate.kind !== "refuse") return;
    expect(gate.reason).toMatch(/terminal is open/);
  });
});
