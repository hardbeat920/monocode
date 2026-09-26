import { describe, expect, it } from "vitest";
import { remoteControlAction } from "../features/remoteControl/model/action";
import {
  autoOpenTargets,
  composerHeld,
  readFailedBecauseMissing,
  takeInjectedEcho,
  COMPOSER_CLEAR,
  forEachSafely,
  createPtyFanout,
  emptyApprovalProgress,
  injectRemoteText,
  queuedNotice,
  remoteApprovalTransition,
  turnSignal,
  type ApprovalEffect,
  type RemoteApprovalView,
  noteInterruptedTurn,
  pendingAfter,
  remoteApprovalKeystroke,
  remoteApprovalReply,
  remoteApprovalView,
  remoteControlName,
  remoteControlStep,
  remoteControlTarget,
  remoteControlTargetForRunningHandover,
  remoteSendGate,
  seatRemoteUserMessage,
  handoverTiming,
  shouldAutoOpen,
  dismissalsAfterModeChange,
  planRemoteExit,
  withClaim,
} from "./remoteControlSession";
import { newSession, type Session } from "../features/sessions/model/session";
import type {
  PermissionPrompt,
  PromptScreen,
} from "../features/remoteControl/model/promptScreen";
import {
  createMirrorState,
  mapRecord,
  resolveTurnFromScreen,
} from "../features/remoteControl/model/transcript";

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
    expect(remoteControlName("/repo/monocode", ["monocode 2"])).toBe(
      "monocode",
    );
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
      automatic: false,
      queued: false,
      busy: false,
    });
  });

  // Neither is on the session — the workspace knows the mode and the queue, so
  // they are passed in rather than guessed at from the thread.
  // Off the session, not passed in: `handoverTiming` decides from the same field,
  // so a second opinion here could disagree with the rule the click is judged by.
  it("takes busy from the session itself", () => {
    expect(
      remoteControlTarget(
        session({ providerSessionId: "sess-abc", busy: true }),
        false,
      ),
    ).toMatchObject({ busy: true });
  });

  it("carries what the workspace knows about waiting", () => {
    expect(
      remoteControlTarget(session({ providerSessionId: "sess-abc" }), false, {
        automatic: true,
        queued: true,
      }),
    ).toMatchObject({ automatic: true, queued: true });
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

  it("seats a message that is only an image", () => {
    // `mapUser` goes out of its way to emit these — it was written because
    // requiring text dropped 55 of them across 161 transcripts, each leaving a
    // reply with nothing above it. Requiring text here dropped them again.
    const next = seatRemoteUserMessage(session(), {
      type: "remote.userMessage",
      text: "",
      attachments: [{ blockType: "image", mediaType: "image/jpeg" }],
    });

    const block = next.blocks.at(-1);
    expect(block?.role).toBe("user");
    expect(block?.text).toBe("[image]");
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

describe("whether a failed read means the file is absent", () => {
  it("recognises what each platform says about a missing file", () => {
    for (const text of [
      "/Users/n/.claude/projects/x/y.jsonl: No such file or directory (os error 2)",
      "C:\\Users\\n\\y.jsonl: The system cannot find the file specified. (os error 2)",
      "C:\\Users\\n\\y.jsonl: The system cannot find the path specified. (os error 3)",
    ]) {
      expect(readFailedBecauseMissing(new Error(text))).toBe(true);
    }
  });

  it("does not call anything else absent", () => {
    // The distinction the hand-over rests on: a refusal or a permission error
    // is a measurement we did not make, and must not become a measurement of
    // zero — that would replay the whole conversation as new events.
    for (const text of [
      "File is too large to open as text",
      "/Users/n/y.jsonl: Permission denied (os error 13)",
      "Not a file",
    ]) {
      expect(readFailedBecauseMissing(new Error(text))).toBe(false);
    }
  });
});

describe("a message of ours coming back through the transcript", () => {
  it("is consumed rather than seated a second time", () => {
    const pending = ["try the other branch"];

    expect(takeInjectedEcho(pending, "  try the other branch  ")).toBe(true);
    expect(pending).toEqual([]);
  });

  it("spends each send once, so the same words twice seat twice", () => {
    const pending = ["again", "again"];

    expect(takeInjectedEcho(pending, "again")).toBe(true);
    expect(takeInjectedEcho(pending, "again")).toBe(true);
    expect(takeInjectedEcho(pending, "again")).toBe(false);
  });

  it("seats a message from the phone", () => {
    // Nothing was typed here, so nothing is owed an echo and the message is
    // somebody else's.
    expect(takeInjectedEcho([], "sent from the phone")).toBe(false);
    expect(takeInjectedEcho(["something else"], "sent from the phone")).toBe(
      false,
    );
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

describe("a thread on its way out", () => {
  const leaving = session({ providerSessionId: "sess-abc", busy: false });

  it("is not handed over while it is being removed", () => {
    // The window between archiving the record and taking the session out of the
    // workspace: gone from the sidebar, still here, idle and bound — which is
    // this rule's yes, and the hand-over it made had nobody left to close it.
    expect(
      shouldAutoOpen(leaving, {
        mode: "all",
        open: false,
        dismissed: false,
        removing: true,
      }),
    ).toBe(false);
  });

  it("is handed over when it is not", () => {
    expect(
      shouldAutoOpen(leaving, {
        mode: "all",
        open: false,
        dismissed: false,
        removing: false,
      }),
    ).toBe(true);
  });
});

describe("a hand-over against a thread the workspace has not loaded", () => {
  it("still offers to close it", () => {
    // The state this exists for: tab closed, so the thread is a history summary
    // with no harness and no bound id, while the CLI is still running and still
    // listed on the phone. An absent entry left nothing able to stop it.
    const action = remoteControlAction(remoteControlTargetForRunningHandover());

    expect(action).toMatchObject({
      id: "remote-control",
      intent: "close",
      disabled: false,
    });
    expect(action?.label).toMatch(/Close/);
  });
});

describe("when a hand-over asked for by hand may run", () => {
  it("waits for a running turn instead of ending it", () => {
    // The loss `noteInterruptedTurn` apologises for is the one this avoids: the
    // click asks for a hand-over, not for the turn to be thrown away.
    expect(handoverTiming(session({ busy: true }))).toBe("when-the-turn-ends");
  });

  it("runs now when nothing is running", () => {
    expect(handoverTiming(session({ busy: false }))).toBe("now");
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
      shouldAutoOpen(
        { ...ready, busy: true },
        {
          mode: "all",
          open: false,
          dismissed: false,
        },
      ),
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

describe("claiming a session for the length of a hand-over", () => {
  it("holds the claim while it runs and releases it after", async () => {
    const claimed = new Set<string>();
    let heldDuringRun = false;
    await withClaim(
      claimed,
      "s1",
      async () => {
        heldDuringRun = claimed.has("s1");
      },
      () => expect.unreachable("should not have failed"),
    );
    expect(heldDuringRun).toBe(true);
    expect(claimed.has("s1")).toBe(false);
  });

  // The whole bug: the release was spelled out on two paths, so a throw anywhere
  // else stranded the id and `shouldAutoOpen` read it as `open` for ever.
  it("releases the claim when the hand-over throws", async () => {
    const claimed = new Set<string>();
    const errors: unknown[] = [];
    await withClaim(
      claimed,
      "s1",
      async () => {
        throw new Error("no such session to resume");
      },
      (error) => errors.push(error),
    );
    expect(claimed.has("s1")).toBe(false);
    expect((errors[0] as Error).message).toBe("no such session to resume");
  });

  // `forEachSafely` wraps the call, not the settlement, so a rejected promise
  // reaches no handler unless this one reports it.
  it("reports rather than rejecting", async () => {
    const claimed = new Set<string>();
    let reported = false;
    await expect(
      withClaim(
        claimed,
        "s1",
        () => Promise.reject(new Error("spawn failed")),
        () => {
          reported = true;
        },
      ),
    ).resolves.toBeUndefined();
    expect(reported).toBe(true);
  });

  it("releases the claim even if the report itself throws", async () => {
    const claimed = new Set<string>();
    await expect(
      withClaim(
        claimed,
        "s1",
        () => Promise.reject(new Error("spawn failed")),
        () => {
          throw new Error("reporting broke too");
        },
      ),
    ).rejects.toThrow("reporting broke too");
    expect(claimed.has("s1")).toBe(false);
  });

  it("does nothing for a hand-over already in flight", async () => {
    const claimed = new Set<string>();
    let runs = 0;
    let second: Promise<void> | null = null;
    await withClaim(
      claimed,
      "s1",
      async () => {
        runs += 1;
        // Re-entered from inside the first, which is exactly when `all` mode did
        // it: the effect re-ran before the finished entry existed.
        second = withClaim(
          claimed,
          "s1",
          async () => {
            runs += 1;
          },
          () => expect.unreachable("should not have been reached"),
        );
        await second;
      },
      () => expect.unreachable("should not have failed"),
    );
    expect(runs).toBe(1);
    expect(claimed.has("s1")).toBe(false);
  });
});

describe("a remote-control pty exiting on its own", () => {
  const ready = session({ providerSessionId: "sess-abc", busy: false });

  it("reports an unclean exit with its code", () => {
    const plan = planRemoteExit(1);
    expect(plan.notice).toBe("error");
    expect(plan.message).toContain("exited with code 1");
  });

  it("reports a signal without inventing a code", () => {
    const plan = planRemoteExit(null);
    expect(plan.notice).toBe("error");
    expect(plan.message).toContain("was terminated");
    expect(plan.message).not.toContain("code");
  });

  // Quitting the CLI in the terminal is not a fault, so it is not raised as one.
  it("notes a clean exit rather than raising it", () => {
    const plan = planRemoteExit(0);
    expect(plan.notice).toBe("status");
    expect(plan.message).toContain("ended");
  });

  it("says how to get it back, whichever way it went", () => {
    for (const code of [0, 1, null]) {
      expect(planRemoteExit(code).message).toContain(
        "open Remote Control again",
      );
    }
  });

  // Not politeness: `all` retaking it spawns again, and the notice changes
  // `sessions`, which re-runs the effect, which spawns again.
  it("stops all mode retaking it, however it exited", () => {
    for (const code of [0, 1, null]) {
      const plan = planRemoteExit(code);
      expect(plan.dismissed).toBe(true);
      expect(
        shouldAutoOpen(ready, {
          mode: "all",
          open: false,
          dismissed: plan.dismissed,
        }),
      ).toBe(false);
    }
  });
});

describe("dismissals across a change of mode", () => {
  const ready = session({ providerSessionId: "sess-abc", busy: false });

  it("clears them on the way into all", () => {
    expect(dismissalsAfterModeChange("manual", "all")).toBe("clear");
  });

  // Otherwise trying the menu by hand first — which is how anyone finds the
  // setting — permanently excludes that session from the mode they then chose.
  it("keeps them while all stays on", () => {
    expect(dismissalsAfterModeChange("all", "all")).toBe("keep");
  });

  it("keeps them on the way out of all", () => {
    expect(dismissalsAfterModeChange("all", "manual")).toBe("keep");
  });

  it("keeps them when manual was and stays the mode", () => {
    expect(dismissalsAfterModeChange("manual", "manual")).toBe("keep");
  });

  it("reopens what was dismissed once the change has cleared it", () => {
    const before = { mode: "all" as const, open: false, dismissed: true };
    expect(shouldAutoOpen(ready, before)).toBe(false);
    const cleared =
      dismissalsAfterModeChange("manual", "all") === "clear"
        ? { ...before, dismissed: false }
        : before;
    expect(shouldAutoOpen(ready, cleared)).toBe(true);
  });
});

const OPTIONS = [
  { number: 1, label: "Yes", keystroke: "1", selected: true },
  {
    number: 2,
    label: "Yes, allow all edits during this session",
    keystroke: "2",
    selected: false,
  },
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
      deny: { label: "Esc to cancel", keystroke: "\x1b" },
      ...patch,
    },
  };
}

const idleScreen: PromptScreen = { lines: ["❯"], turn: "ended", kind: "idle" };

describe("what a remote session shows for a pending prompt", () => {
  it("shows the prompt when the transcript says something is waiting", () => {
    const view = remoteApprovalView({
      pending: true,
      screen: promptScreen(),
      terminalOpen: false,
    });

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
    // Named `deny` after the parser's rename: Esc denies rather than dismissing.
    expect(view.deny).toBe("Esc to cancel");
  });

  it("shows nothing when the transcript says nothing is waiting", () => {
    // A screen that looks like a prompt is not evidence that one is pending —
    // this is the composer-footer false positive the probe hit.
    expect(
      remoteApprovalView({
        pending: false,
        screen: promptScreen(),
        terminalOpen: false,
      }),
    ).toEqual({ kind: "none" });
  });

  it("stays quiet while a tool is merely running", () => {
    // An outstanding tool call is equally the ordinary state of a running tool,
    // so an idle or busy screen must not raise anything.
    expect(
      remoteApprovalView({
        pending: true,
        screen: idleScreen,
        terminalOpen: false,
      }),
    ).toEqual({ kind: "none" });
  });

  it.each([
    [
      "a dialog it cannot identify",
      {
        lines: ["1. Something", "2. Else"],
        turn: "in-progress",
        kind: "unrecognised",
        reason: "unknown-dialog",
      } as PromptScreen,
    ],
    [
      "a modal it must not answer",
      {
        lines: ["Is this a project you created or one you trust?"],
        turn: "unknown",
        kind: "modal",
        modal: {
          question: "Is this a project you created or one you trust?",
          detail: [],
          options: ["1. Yes"],
          footer: "Enter to confirm",
        },
      } as PromptScreen,
    ],
  ])("surfaces the raw screen for %s", (_label, screen) => {
    const view = remoteApprovalView({
      pending: true,
      screen,
      terminalOpen: false,
    });

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
      remoteApprovalKeystroke(promptScreen({ deny: undefined }), {
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
      {
        type: "tool.updated",
        callId: "t1",
        title: "Write",
        status: "completed",
      },
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

const allowed = {
  allowed: true,
  bytes: "\x1b[200~say OK\x1b[201~\r",
  queued: false,
} as const;

/**
 * An idle composer as the parser actually reports one since #30: `kind: "idle"`
 * with `turn: "unknown"`, because a composer with no spinner over it says nothing
 * about the turn either way.
 */
function idleWith(lines: readonly string[]): PromptScreen {
  return { lines, turn: "unknown", kind: "idle" };
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

  it("ignores a user message sitting in the scrollback", () => {
    // `❯` starts every user message the CLI has already printed, so a screen
    // with any history above the box has several of them. Reading the first
    // one made every send fail — cleared, looked again, found the scrollback
    // row unchanged, refused — and told `turnSignal` a composer was held,
    // which ends a running turn after three seconds of quiet.
    const withHistory = [
      "❯ an earlier message, already sent",
      "⏺ and the reply to it",
      ...IDLE_COMPOSER,
    ];

    expect(composerHeld(withHistory)).toBeNull();
  });

  it("still reads the box when history sits above it", () => {
    const withHistory = [
      "❯ an earlier message, already sent",
      ...AFTER_INTERRUPT,
    ];

    expect(composerHeld(withHistory)).toBe(
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

describe("one pty subscription, many readers", () => {
  it("gives the parser every chunk", () => {
    const parsed: string[] = [];
    const fanout = createPtyFanout((chunk) => parsed.push(chunk));

    fanout.dispatch("a");
    fanout.dispatch("b");

    expect(parsed).toEqual(["a", "b"]);
  });

  it("gives an attached viewer the chunks after it attached", () => {
    const seen: string[] = [];
    const fanout = createPtyFanout(() => undefined);

    fanout.dispatch("before");
    fanout.attach((chunk) => seen.push(chunk));
    fanout.dispatch("after");

    expect(seen).toEqual(["after"]);
  });

  it("keeps the parser subscribed when a viewer detaches", () => {
    // The regression this exists for: `subscribePty` holds one handler per id,
    // so if a view's teardown could remove the parser the pty would be left
    // watched by nobody — silently, with no error anywhere.
    const parsed: string[] = [];
    const fanout = createPtyFanout((chunk) => parsed.push(chunk));
    const detach = fanout.attach(() => undefined);

    detach();
    fanout.dispatch("still parsed");

    expect(parsed).toEqual(["still parsed"]);
    expect(fanout.viewerCount()).toBe(0);
  });

  it("detaches exactly one viewer", () => {
    const first: string[] = [];
    const second: string[] = [];
    const fanout = createPtyFanout(() => undefined);
    const detachFirst = fanout.attach((chunk) => first.push(chunk));
    fanout.attach((chunk) => second.push(chunk));

    detachFirst();
    fanout.dispatch("x");

    expect(first).toEqual([]);
    expect(second).toEqual(["x"]);
    expect(fanout.viewerCount()).toBe(1);
  });

  it("is harmless to detach twice", () => {
    const fanout = createPtyFanout(() => undefined);
    const detach = fanout.attach(() => undefined);
    fanout.attach(() => undefined);

    detach();
    detach();

    expect(fanout.viewerCount()).toBe(1);
  });

  it("keeps parsing when a viewer throws", () => {
    // A disposed terminal throws on write. The subscription has to survive it,
    // and the parser has already had the chunk by then.
    const parsed: string[] = [];
    const fanout = createPtyFanout((chunk) => parsed.push(chunk));
    fanout.attach(() => {
      throw new Error("disposed");
    });
    const survivor: string[] = [];
    fanout.attach((chunk) => survivor.push(chunk));

    expect(() => fanout.dispatch("x")).not.toThrow();
    expect(parsed).toEqual(["x"]);
    expect(survivor).toEqual(["x"]);
  });
});

/** A screen sequence the injector walks through as it polls. */
function scriptedPty(screens: (PromptScreen | null)[]) {
  const written: string[] = [];
  let at = 0;
  return {
    written,
    ports: {
      write: async (bytes: string) => {
        written.push(bytes);
      },
      screen: () => screens[Math.min(at, screens.length - 1)],
      settle: async () => {
        at += 1;
      },
      attempts: 3,
    },
  };
}

const MESSAGE = "\x1b[200~say OK\x1b[201~\r";

describe("typing a message into the pty", () => {
  it("sends straight away when the composer is empty", async () => {
    const pty = scriptedPty([idleWith(IDLE_COMPOSER)]);

    const outcome = await injectRemoteText("say OK", false, pty.ports);

    // `queued: "unknown"` rather than `false`: an idle composer says nothing
    // about the turn since #30, so whether the TUI queues this is unknowable
    // from the frame — which is what the hedge exists for.
    expect(outcome).toEqual({ kind: "sent", queued: "unknown" });
    expect(pty.written).toEqual([MESSAGE]);
  });

  it("reports a definitely-finished turn as not queued", async () => {
    const screen = {
      ...idleWith(IDLE_COMPOSER),
      turn: "ended",
    } as PromptScreen;
    const pty = scriptedPty([screen]);

    expect(await injectRemoteText("say OK", false, pty.ports)).toEqual({
      kind: "sent",
      queued: false,
    });
  });

  it("clears a restored prompt first, then sends", async () => {
    const pty = scriptedPty([
      idleWith(AFTER_INTERRUPT),
      idleWith(IDLE_COMPOSER),
    ]);

    const outcome = await injectRemoteText("say OK", false, pty.ports);

    expect(outcome.kind).toBe("sent");
    // The clear goes first and the message only after the screen came back
    // empty, which is the whole ordering.
    expect(pty.written).toEqual([COMPOSER_CLEAR, MESSAGE]);
  });

  it("never writes the message while the composer still holds text", async () => {
    // The measured bug: bracketed paste appends, so a send here would submit
    // the interrupted prompt and this message welded together.
    const pty = scriptedPty([idleWith(AFTER_INTERRUPT)]);

    const outcome = await injectRemoteText("say OK", false, pty.ports);

    expect(outcome.kind).toBe("refused");
    if (outcome.kind !== "refused") return;
    expect(outcome.reason).toContain("abacus");
    expect(pty.written).toEqual([COMPOSER_CLEAR]);
    expect(pty.written).not.toContain(MESSAGE);
  });

  it("writes nothing at all when the parser refuses the screen", async () => {
    const pty = scriptedPty([
      {
        lines: ["Do you want to create probe.txt?"],
        turn: "in-progress",
        kind: "permission-prompt",
        prompt: promptScreen().prompt,
      } as PromptScreen,
    ]);

    const outcome = await injectRemoteText("say OK", false, pty.ports);

    expect(outcome.kind).toBe("refused");
    expect(pty.written).toEqual([]);
  });

  it("writes nothing while the user has the terminal open", async () => {
    const pty = scriptedPty([idleWith(IDLE_COMPOSER)]);

    const outcome = await injectRemoteText("say OK", true, pty.ports);

    expect(outcome.kind).toBe("refused");
    expect(pty.written).toEqual([]);
  });

  it("refuses before the terminal has painted", async () => {
    const pty = scriptedPty([null]);

    expect((await injectRemoteText("say OK", false, pty.ports)).kind).toBe(
      "refused",
    );
    expect(pty.written).toEqual([]);
  });
});

describe("describing where a sent message went", () => {
  it("says nothing when it went straight in", () => {
    expect(queuedNotice(false)).toBeNull();
  });

  it("says it was queued when the screen knows a turn is running", () => {
    expect(queuedNotice(true)).toMatch(/Queued/);
  });

  it("hedges when the screen cannot tell", () => {
    // `kind` answers "can I inject"; `turn` answers "will it queue". During
    // streaming the screen reads idle with an unknown turn, so a confident
    // answer either way would be a guess.
    const notice = queuedNotice("unknown");

    expect(notice).toMatch(/may be queued/);
    expect(notice).not.toMatch(/^Queued/);
  });
});

const QUESTION_VIEW = remoteApprovalView({
  pending: true,
  screen: promptScreen(),
  terminalOpen: false,
});

const RAW_VIEW: RemoteApprovalView = {
  kind: "raw",
  lines: ["1. Something"],
  reason: "unknown-dialog",
};

/** Same prompt, fewer options — a box caught halfway through painting. */
function halfPainted(): RemoteApprovalView {
  return remoteApprovalView({
    pending: true,
    terminalOpen: false,
    screen: promptScreen({ options: OPTIONS.slice(0, 2) }),
  });
}

function run(views: RemoteApprovalView[], from = emptyApprovalProgress()) {
  let progress = from;
  const effects: ApprovalEffect[] = [];
  let nextId = 0;
  for (const view of views) {
    const step = remoteApprovalTransition(progress, view, nextId + 1);
    progress = step.progress;
    for (const effect of step.effects) {
      if (effect.kind === "ask") nextId = effect.requestId;
      effects.push(effect);
    }
  }
  return { progress, effects };
}

describe("deciding when an approval goes in front of the user", () => {
  it("does not raise on the first frame", () => {
    // Contiguous numbering with one cursor proves the run is unbroken, not that
    // it is finished, so one frame is never enough.
    const { effects, progress } = run([QUESTION_VIEW]);

    expect(effects).toEqual([]);
    expect(progress.shown).toBeNull();
  });

  it("raises once two frames agree", () => {
    const { effects } = run([QUESTION_VIEW, QUESTION_VIEW]);

    expect(effects).toHaveLength(1);
    expect(effects[0]).toMatchObject({ kind: "ask", requestId: 1 });
  });

  it("never raises a half-painted option list", () => {
    // Two options where the finished prompt has three: an approval with no
    // denial on it, which is the whole reason this gate exists.
    const { effects } = run([halfPainted(), QUESTION_VIEW]);

    expect(effects).toEqual([]);
  });

  it("raises the settled prompt after a half-painted frame", () => {
    const { effects } = run([halfPainted(), QUESTION_VIEW, QUESTION_VIEW]);

    expect(effects).toHaveLength(1);
    const asked = effects[0];
    expect(asked.kind).toBe("ask");
    if (asked.kind !== "ask") return;
    expect(asked.question.options.map((option) => option.label)).toEqual([
      "Yes",
      "Yes, allow all edits during this session",
      "No",
    ]);
  });

  it("does not raise twice for one prompt", () => {
    const { effects } = run([QUESTION_VIEW, QUESTION_VIEW, QUESTION_VIEW]);

    expect(effects.filter((effect) => effect.kind === "ask")).toHaveLength(1);
  });

  it("surfaces the terminal when the options keep changing", () => {
    // A screen repainting is not a prompt being read; it is one we cannot read.
    // Refusing to raise is safe, and it must not be silent.
    const { effects } = run([
      halfPainted(),
      QUESTION_VIEW,
      remoteApprovalView({
        pending: true,
        terminalOpen: false,
        screen: promptScreen({ options: OPTIONS.slice(0, 1) }),
      }),
    ]);

    expect(effects.map((effect) => effect.kind)).toEqual([
      "openTerminal",
      "surface",
    ]);
  });

  it("treats a cursor moving as the same prompt", () => {
    // Navigation within one prompt. Counting selection would mean arrow keys
    // suppressed the raise, and it guards nothing the labels do not.
    const moved = remoteApprovalView({
      pending: true,
      terminalOpen: false,
      screen: promptScreen({
        options: OPTIONS.map((option, index) => ({
          ...option,
          selected: index === 2,
        })),
      }),
    });

    const { effects } = run([QUESTION_VIEW, moved]);

    expect(effects).toHaveLength(1);
    expect(effects[0].kind).toBe("ask");
  });

  it("treats a prompt with no footer as the same prompt", () => {
    // After #38 a missing footer is legitimate, so it cannot distinguish a
    // partial paint from a complete one.
    const noFooter = remoteApprovalView({
      pending: true,
      terminalOpen: false,
      screen: promptScreen({ deny: undefined }),
    });

    const { effects } = run([QUESTION_VIEW, noFooter]);

    expect(effects).toHaveLength(1);
    expect(effects[0].kind).toBe("ask");
  });
});

describe("retiring and re-raising", () => {
  it("retires a prompt answered elsewhere as cancelled, not skipped", () => {
    const shown = run([QUESTION_VIEW, QUESTION_VIEW]).progress;

    const { effects, progress } = run([{ kind: "none" }], shown);

    expect(effects).toEqual([
      { kind: "resolve", requestId: 1, decision: "cancelled" },
    ]);
    expect(progress.shown).toBeNull();
  });

  it("says nothing when there was nothing in front of the user", () => {
    expect(run([{ kind: "none" }]).effects).toEqual([]);
  });

  it("surfaces an unreadable screen once, not on every frame", () => {
    const { effects } = run([RAW_VIEW, RAW_VIEW, RAW_VIEW]);

    expect(effects.map((effect) => effect.kind)).toEqual([
      "openTerminal",
      "surface",
    ]);
  });

  it("re-opens the terminal after it was hidden, without repeating itself", () => {
    // Hiding the terminal with something still unreadable has to put it back —
    // something genuinely is waiting. What it must not do is re-print the screen
    // into the transcript every time, which is what made this a decision worth
    // recording rather than a side effect of the ordering.
    const afterRaw = run([RAW_VIEW]).progress;
    // The view reads `none` while the terminal is open, clearing `shown`.
    const hidden = run([{ kind: "none" }], afterRaw).progress;

    const { effects } = run([RAW_VIEW], hidden);

    expect(effects.map((effect) => effect.kind)).toEqual([
      "openTerminal",
      "surface",
    ]);
  });

  it("allocates a request id only when it actually raises", () => {
    const { effects } = run([
      QUESTION_VIEW,
      QUESTION_VIEW,
      { kind: "none" },
      QUESTION_VIEW,
      QUESTION_VIEW,
    ]);

    const ids = effects.flatMap((effect) =>
      effect.kind === "ask" ? [effect.requestId] : [],
    );
    expect(ids).toEqual([1, 2]);
  });
});

describe("what to tell the mirror about a turn", () => {
  const held = idleWith(AFTER_INTERRUPT);

  it("takes the screen's word when it says the turn ended", () => {
    // `COMPLETED` matched a real marker, so nothing else is considered.
    expect(turnSignal({ ...held, turn: "ended" }, 1_000, 99_000)).toBe(true);
  });

  it("reports silence and a held composer when the screen cannot tell", () => {
    expect(turnSignal(held, 1_000, 6_000)).toEqual({
      quietForMs: 5_000,
      composerHeld: true,
    });
  });

  it("reports an empty composer as such", () => {
    expect(turnSignal(idleWith(IDLE_COMPOSER), 1_000, 6_000)).toEqual({
      quietForMs: 5_000,
      composerHeld: false,
    });
  });

  it("gives no signal when the pty has never produced output", () => {
    // Quiet since a moment nobody observed is not evidence of silence, and an
    // absence of evidence must never end a turn.
    expect(turnSignal(held, null, 99_000)).toBe(false);
  });

  it("gives no signal before the screen has painted", () => {
    expect(turnSignal(null, 1_000, 99_000)).toBe(false);
  });

  it("never reports negative quiet from a clock that moved backwards", () => {
    expect(turnSignal(held, 9_000, 1_000)).toEqual({
      quietForMs: 0,
      composerHeld: true,
    });
  });
});

describe("an interrupt resolves with no further output", () => {
  /** A live turn, opened by a user record the way the mirror opens one. */
  function turnInFlight() {
    const state = createMirrorState();
    mapRecord(state, {
      type: "user",
      message: { role: "user", content: [{ type: "text", text: "go" }] },
    });
    expect(state.turn.active).toBe(true);
    return state;
  }

  const held = idleWith(AFTER_INTERRUPT);
  const spokeAt = 1_000;

  it("ends the turn once the clock passes the threshold, with no chunk", () => {
    // Every other test in this file proves what happens when something arrives.
    // This is the one rule whose trigger is nothing arriving, so an event-driven
    // suite would pass whether or not the caller ever re-evaluates.
    const state = turnInFlight();

    const events = resolveTurnFromScreen(
      state,
      turnSignal(held, spokeAt, spokeAt + 3_500),
    );

    expect(state.turn.active).toBe(false);
    expect(events).toEqual([]);
  });

  it("leaves the turn running while the silence is still short", () => {
    // 800ms is the measured worst case between reads during a live turn, so a
    // threshold reached early would end turns that are merely thinking.
    const state = turnInFlight();

    resolveTurnFromScreen(state, turnSignal(held, spokeAt, spokeAt + 900));

    expect(state.turn.active).toBe(true);
  });

  it("leaves the turn running when the composer is empty", () => {
    // Silence alone is not an interrupt: the restored prompt is the positive
    // trace, and without it this is just a quiet turn.
    const state = turnInFlight();

    resolveTurnFromScreen(
      state,
      turnSignal(idleWith(IDLE_COMPOSER), spokeAt, spokeAt + 3_500),
    );

    expect(state.turn.active).toBe(true);
  });

  it("leaves the turn running when the pty has said nothing yet", () => {
    const state = turnInFlight();

    resolveTurnFromScreen(state, turnSignal(held, null, spokeAt + 99_000));

    expect(state.turn.active).toBe(true);
  });
});

describe("opening every session at once", () => {
  const ready = (id: string) => ({
    ...newSession("claude", "/repo/monocode"),
    id,
    providerSessionId: `sess-${id}`,
    busy: false,
  });

  it("keeps going when one session throws", () => {
    // This ran inside an effect across every thread, so a throw for one session
    // abandoned the rest and took the React tree with it.
    const opened: string[] = [];
    const failed: string[] = [];

    forEachSafely(
      ["a", "b", "c"],
      (id) => {
        if (id === "b") throw new Error("no transcript");
        opened.push(id);
      },
      (id) => failed.push(id),
    );

    expect(opened).toEqual(["a", "c"]);
    expect(failed).toEqual(["b"]);
  });

  it("reports the failure rather than swallowing it", () => {
    const seen: unknown[] = [];

    forEachSafely(
      ["a"],
      () => {
        throw new Error("boom");
      },
      (_id, error) => seen.push(error),
    );

    expect(seen).toHaveLength(1);
    expect((seen[0] as Error).message).toBe("boom");
  });

  it("picks the eligible sessions and no others", () => {
    const sessions = [
      ready("a"),
      { ...ready("b"), busy: true },
      { ...newSession("codex", "/repo/other"), id: "c" },
    ];

    expect(
      autoOpenTargets(sessions, () => ({
        mode: "all",
        open: false,
        dismissed: false,
      })),
    ).toEqual(["a"]);
  });

  it("skips a session whose hand-over has started but not finished", () => {
    // The bug: the finished entry is written several awaits after the start, so
    // treating "not finished" as "not started" re-entered the hand-over on every
    // re-render — unboundedly, until React tore the tree down.
    const inFlight = new Set(["a"]);
    const sessions = [ready("a"), ready("b")];

    expect(
      autoOpenTargets(sessions, (session) => ({
        mode: "all",
        open: inFlight.has(session.id),
        dismissed: false,
      })),
    ).toEqual(["b"]);
  });
});
