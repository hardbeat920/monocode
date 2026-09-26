/**
 * The decisions behind wiring Remote Control into the workspace, kept out of
 * `App.tsx` so they can be tested without rendering it.
 *
 * Nothing here spawns, kills or reads anything — the effects live at the call
 * site in `App.tsx`, and these are the questions it has to answer first: what to
 * call a session on a phone, how an inbound message is seated, and when `all`
 * mode may take a conversation over.
 */

import type {
  RemoteControlIntent,
  RemoteControlTarget,
} from "../features/remoteControl/model/action";
import { planInjection } from "../features/remoteControl/model/promptScreen";
import type {
  InjectionPlan,
  InjectionRefusal,
  PermissionPrompt,
  PromptOption,
  PromptScreen,
} from "../features/remoteControl/model/promptScreen";
import type {
  MirrorEvent,
  RemoteUserMessage,
  TurnLiveness,
} from "../features/remoteControl/model/transcript";
import type {
  UserQuestion,
  UserQuestionReply,
} from "../features/sessions/model/userQuestion";
import type { RemoteControlMode } from "../features/settings/model/settings";
import type { Session } from "../features/sessions/model/session";
import { projectName } from "../shared/lib/paths";

export function remoteControlTarget(
  session: Session,
  active: boolean,
  /**
   * What the workspace knows and the session does not: whether the mode hands
   * threads over on its own, and whether this one is already waiting its turn.
   */
  waiting: { automatic: boolean; queued: boolean } = {
    automatic: false,
    queued: false,
  },
): RemoteControlTarget {
  return {
    harness: session.harness,
    providerSessionId: session.providerSessionId,
    active,
    automatic: waiting.automatic,
    queued: waiting.queued,
    // Read off the session rather than passed in: `handoverTiming` decides from
    // the same field, so a second opinion here could disagree with the rule the
    // click will actually be judged by.
    busy: session.busy ?? false,
  };
}

/**
 * The target for a hand-over this window is running against a thread it has not
 * loaded.
 *
 * The workspace-backed rule needs a `Session`, and a thread whose tab has been
 * closed is only a history summary carrying neither harness nor
 * `providerSessionId` — so it returned nothing, and the entry vanished from the
 * menu while the CLI went on running and stayed listed on the phone. Unreachable
 * and alive is the worst of the states: the user cannot even see what to stop.
 *
 * Closing needs none of what the summary lacks. `active` is what a close turns
 * on, the id only matters for opening, and a live hand-over is only ever Claude —
 * nothing else can produce one. So this is the whole target, and it exists to say
 * "you can still close this".
 */
export function remoteControlTargetForRunningHandover(): RemoteControlTarget {
  return { harness: "claude", active: true };
}

/**
 * What to actually do about a click, given the state now.
 *
 * The menu decided the intent from what it could see when it opened. If that
 * disagrees with the state now — an `all`-mode open landed in between, or the
 * thread was archived — the click was aimed at something that has since changed,
 * so nothing happens. Flipping to the other action instead would carry out an
 * instruction the user never gave.
 */
export function remoteControlStep(
  intent: RemoteControlIntent,
  open: boolean,
): "open" | "close" | "none" {
  if (intent === "open") return open ? "none" : "open";
  return open ? "close" : "none";
}

/**
 * What the session is called on a phone.
 *
 * Assigned once, when the process starts, and never revisited: the CLI cannot be
 * renamed after launch, so a name derived from how many threads exist would
 * describe the wrong thread by the time someone read it. Hence the counter is
 * resolved against the names already in use rather than against a count.
 */
export function remoteControlName(
  cwd: string,
  taken: Iterable<string>,
): string {
  const base = projectName(cwd).trim() || "session";
  const used = new Set(taken);
  if (!used.has(base)) return base;
  let n = 2;
  while (used.has(`${base} ${n}`)) n += 1;
  return `${base} ${n}`;
}

/**
 * Seat a message that arrived from the phone or the TUI.
 *
 * It is seated as an ordinary user block because that is what it is: a user turn
 * on this conversation that did not come from this composer. `interjection` —
 * the only existing event carrying free text inward — appends a *system* block,
 * which would read as MonoCode remarking on the conversation rather than as part
 * of it. So the shape here matches what the composer itself appends.
 */
export function seatRemoteUserMessage(
  session: Session,
  event: RemoteUserMessage,
  now = Date.now(),
): Session {
  const text = event.text.trim();
  if (!text) return session;
  return {
    ...session,
    blocks: [
      ...session.blocks,
      { id: crypto.randomUUID(), role: "user", text, startedAt: now },
    ],
  };
}

/**
 * Tell the user their turn stopped, when opening Remote Control interrupted one.
 *
 * The hand-over stops the headless child, which ends whatever it was doing. A
 * turn still inside startup is superseded by the stop epoch and returns without
 * an error of its own, so nothing else would say anything and the user would
 * watch their message sit there unanswered. A turn already streaming fails its
 * next write and reports that, in which case this block explains a failure that
 * would otherwise look unprovoked.
 */
export function noteInterruptedTurn(session: Session): Session {
  if (!session.busy) return session;
  return {
    ...session,
    blocks: [
      ...session.blocks,
      {
        id: crypto.randomUUID(),
        role: "system",
        text: "Opening Remote Control stopped the turn that was running. Send it again from the phone, or close Remote Control and send it here.",
        notice: "error",
      },
    ],
  };
}

export type AutoOpenState = {
  mode: RemoteControlMode;
  /** Already handed over. */
  open: boolean;
  /** Closed by hand. `all` must not argue with the user about it. */
  dismissed: boolean;
  /**
   * The thread is on its way out — being archived or deleted.
   *
   * Removal stops the child, saves it, and archives the record before it takes
   * the session out of the workspace, reading the state again after each wait.
   * So for a moment the thread is gone from the sidebar and still in the
   * workspace: idle, with a conversation bound, which is exactly this rule's
   * yes. The hand-over it produced then belonged to nobody — the close ran at
   * the start of the removal, before this could happen, so nothing was left to
   * shut the new CLI down and it stayed open.
   *
   * The by-hand close does mark the thread dismissed on its way past, which
   * happens to block this too. That is a side effect of what `byUser` means
   * today, not a guarantee: dismissal is about the user's intent and this is
   * about the lifecycle, and leaning on the first to enforce the second breaks
   * the day someone changes what a removal's close counts as.
   */
  removing?: boolean;
};

/**
 * Whether `all` mode may take this session over now.
 *
 * "Lazily" is pinned to the session going idle with a conversation bound, which
 * is not a preference but the only moment that satisfies the hand-over's own
 * constraints. Before a first turn there is no `providerSessionId`, so there is
 * nothing for `--resume` to open. During a turn the hand-over would stop the
 * child the user is waiting on. And because a thread nobody has used never
 * reaches either state, processes accumulate against *used* threads rather than
 * open tabs — which is the cost §9.6 asks to avoid.
 */
export type RemoteExitPlan = {
  /** Whether `all` mode may hand this session over again unasked. */
  dismissed: boolean;
  /** `error` for an exit nobody asked for; a clean one is only worth a note. */
  notice: "error" | "status";
  message: string;
};

/**
 * What to do when a remote-control pty exits on its own.
 *
 * Tearing the entry down is not optional, and nothing decides it here because
 * there is nothing to decide: `remoteControlIds` is what every menu reads for
 * "already active" and what `shouldAutoOpen` reads for `open`, so an entry left
 * behind makes every menu offer **Close** for a process that is gone and locks
 * `all` mode out of that session for the life of the window. Both were observed.
 *
 * What is decided here is that it counts as **dismissed**, for two different
 * reasons that happen to agree. A clean exit is the user quitting the CLI
 * themselves, and `all` retaking it would be arguing with them. An unclean one
 * is worse than that: `all` retaking it spawns again, and the notice below
 * changes `sessions`, which re-runs the effect, which spawns again — the same
 * unbounded respawn `3b2fc33` was written to stop. Nothing here has any evidence
 * the next attempt would fare better, so it stops, says why, and leaves the
 * retry to the user.
 */
export function planRemoteExit(code: number | null): RemoteExitPlan {
  const tail =
    " The thread carries on here; open Remote Control again to hand it back over.";
  if (code === 0) {
    return {
      dismissed: true,
      notice: "status",
      message: `Remote Control ended — the interactive session exited.${tail}`,
    };
  }
  const how = code === null ? "was terminated" : `exited with code ${code}`;
  return {
    dismissed: true,
    notice: "error",
    message: `Remote Control stopped — the interactive session ${how}.${tail}`,
  };
}

/**
 * Whether the by-hand dismissals survive a change of mode.
 *
 * `dismissed` exists so `all` does not argue with a close the user made while it
 * was on, and within one mode that is right. Across a change into `all` it is
 * backwards: picking Every session is a *later* instruction than any close that
 * came before it, so those closes have been superseded. Keeping them means the
 * setting the user just chose silently skips the very sessions they had already
 * tried by hand — which, for anyone who reached the setting by trying the menu
 * first, is every session they have.
 *
 * Only on the way *in*. Leaving `all` and coming back is the same instruction
 * being given again, and a close made while `all` was on is the case `dismissed`
 * was written for.
 */
export function dismissalsAfterModeChange(
  previous: RemoteControlMode,
  next: RemoteControlMode,
): "keep" | "clear" {
  return next === "all" && previous !== "all" ? "clear" : "keep";
}

/** When a hand-over asked for by hand may run. */
export type HandoverTiming = "now" | "when-the-turn-ends";

/**
 * Whether opening Remote Control by hand may run now, or has to wait.
 *
 * The hand-over stops the headless child, so running it mid-turn ends the turn
 * the user is waiting on — which is what `noteInterruptedTurn` exists to
 * apologise for. Waiting is strictly better and costs nothing: the hand-over's
 * own precondition is a session idle with a conversation bound, and the end of
 * the turn is exactly what produces that. So a click during a turn is a request
 * to hand over, not a request to lose the turn.
 *
 * `all` mode already waits, by way of `shouldAutoOpen`. This gives the by-hand
 * path the same rule, which is also why the two share one queue at the call
 * site.
 */
export function handoverTiming(session: Session): HandoverTiming {
  return session.busy ? "when-the-turn-ends" : "now";
}

export function shouldAutoOpen(
  session: Session,
  state: AutoOpenState,
): boolean {
  if (state.mode !== "all") return false;
  if (state.open || state.dismissed || state.removing) return false;
  if (session.harness !== "claude") return false;
  if (!session.providerSessionId) return false;
  return !session.busy;
}

// --------------------------------------------------------------- approvals

/**
 * What a remote-controlled session should show for a pending permission prompt.
 *
 * Two sources, each answering the question it can. **Whether** something is
 * waiting comes from the transcript — a `tool_use` with no `tool_result` behind
 * it, which held across every measured run. **What it says** comes from the
 * screen, because permission prompts are written nowhere else. Deciding
 * *whether* from the screen is how a probe matched the composer's own
 * "Esc to cancel" footer 0.1s in and measured nothing.
 *
 * Splitting them this way produces the fail-safe state deliberately rather than
 * by accident: transcript says pending, screen will not parse, so we know
 * something is waiting and cannot read it — which is exactly when the decided
 * rule is to surface the raw pty instead of answering for the user.
 */
export type RemoteApprovalView =
  | { kind: "none" }
  | {
      kind: "question";
      question: UserQuestion;
      deny?: string;
      /** Carried so an option set that never settles can be shown verbatim. */
      lines: readonly string[];
    }
  /** Something is waiting and the screen could not be read. Show it verbatim. */
  | { kind: "raw"; lines: readonly string[]; reason: string };

export type ApprovalInputs = {
  /** An outstanding tool call, from the transcript. */
  pending: boolean;
  screen: PromptScreen;
  /**
   * The user has the terminal open and the keyboard with it. MonoCode raises
   * nothing then: two surfaces for one prompt invites two answers, and the
   * second digit lands on whatever screen the first one produced.
   */
  terminalOpen: boolean;
};

export function remoteApprovalView({
  pending,
  screen,
  terminalOpen,
}: ApprovalInputs): RemoteApprovalView {
  if (terminalOpen) return { kind: "none" };
  // No outstanding tool call means nothing is waiting on an answer, whatever the
  // screen happens to look like.
  if (!pending) return { kind: "none" };
  if (screen.kind === "permission-prompt") {
    return {
      kind: "question",
      question: questionFromPrompt(screen.prompt),
      lines: screen.lines,
      ...(screen.prompt.deny ? { deny: screen.prompt.deny.label } : {}),
    };
  }
  // An outstanding call is equally the ordinary state of a tool that is simply
  // running, so an unreadable screen is not on its own evidence of a question.
  // These two kinds are: `unknown-dialog` means numbered options are painted
  // inside a dialog the parser does not know, and a modal is deliberately parsed
  // without keystrokes so that MonoCode cannot answer it. Both mean something is
  // asking and we cannot read it, which is when the raw pty goes in front of the
  // user rather than a guess.
  if (screen.kind === "modal") {
    return { kind: "raw", lines: screen.lines, reason: "modal" };
  }
  if (screen.kind === "unrecognised" && screen.reason === "unknown-dialog") {
    return { kind: "raw", lines: screen.lines, reason: screen.reason };
  }
  return { kind: "none" };
}

/** The id of the option a digit stands for. Stable, and what gets injected. */
function optionId(option: PromptOption): string {
  return String(option.number);
}

/**
 * The prompt as one of MonoCode's own clarifying questions.
 *
 * `question.asked` is the only inbound shape that carries more than two answers
 * and a way out that is not an answer — `{kind:"skipped"}`. `approval.requested`
 * cannot: `ApprovalDecision` is `"allow" | "deny"`, which cannot express
 * "Yes, allow all edits during this session", and offering a prompt without its
 * cancel narrows a safe parser into a coercive one.
 *
 * The tool header and diff ride in the prompt text because they are the only
 * statement anywhere of what the prompt would *do* — the question names the file
 * and not its contents, and the transcript names neither. A UI without them asks
 * for approval of a write nobody can see.
 */
function questionFromPrompt(prompt: PermissionPrompt): UserQuestion {
  const detail = prompt.detail.filter((line) => line.trim());
  const [header, ...rest] = detail;
  return {
    id: "remote-approval",
    ...(header ? { header } : {}),
    prompt:
      rest.length > 0
        ? `${prompt.question}\n\n${rest.join("\n")}`
        : prompt.question,
    multiSelect: false,
    allowCustom: false,
    options: prompt.options.map((option) => ({
      id: optionId(option),
      label: option.label,
    })),
  };
}

export type RemoteAnswer =
  /** A numbered option, by the id `remoteApprovalView` gave it. */
  | { kind: "option"; id: string }
  /** The prompt's own way out. Esc, when the footer advertises it. */
  | { kind: "cancel" };

/**
 * The bytes that answer a prompt, or `null` when nothing may be sent.
 *
 * A bare digit both selects and acts — measured, with no cursor-move step and no
 * `\r`; a trailing CR is 80 bytes of mouse-mode housekeeping and no second
 * action. An out-of-range digit is a clean no-op that leaves the prompt pending,
 * which is a good failure and still not one worth causing: an option that is not
 * on screen never becomes a keystroke, and neither does an answer to a screen
 * that is not a prompt.
 */
export function remoteApprovalKeystroke(
  screen: PromptScreen,
  answer: RemoteAnswer,
): string | null {
  if (screen.kind !== "permission-prompt") return null;
  if (answer.kind === "cancel") {
    return screen.prompt.deny ? screen.prompt.deny.keystroke : null;
  }
  const option = screen.prompt.options.find(
    (candidate) => optionId(candidate) === answer.id,
  );
  return option ? option.keystroke : null;
}

/**
 * What to record as the outcome, decided here rather than read back later.
 *
 * Esc produces a `tool_result` byte-identical to option 3's — `is_error: true`,
 * `"User rejected tool use"` — so the transcript cannot tell a cancel from an
 * explicit No, while MonoCode distinguishes `deny` from `cancelled`. The client
 * that sent the keystroke is the only thing that knows, so the answer is
 * captured at the moment of injection.
 */
export function remoteApprovalReply(answer: RemoteAnswer): UserQuestionReply {
  if (answer.kind === "cancel") return { kind: "skipped" };
  return { kind: "answered", answers: { "remote-approval": [answer.id] } };
}

/**
 * Outstanding tool calls after a batch of mirrored events.
 *
 * Derived from the events the mirror already emits rather than by reading the
 * records again: `tool.started` opens a call and `tool.updated` closes it with a
 * terminal status. `MirrorState.tools` cannot answer this — it is only ever
 * added to, so "still in the map" is true forever.
 */
export function pendingAfter(
  pending: ReadonlySet<string>,
  events: readonly MirrorEvent[],
): Set<string> {
  const next = new Set(pending);
  for (const event of events) {
    if (event.type === "tool.started" && event.callId) next.add(event.callId);
    if (
      event.type === "tool.updated" &&
      event.callId &&
      (event.status === "completed" || event.status === "failed")
    ) {
      next.delete(event.callId);
    }
  }
  return next;
}

/** Why a composer send did not reach the pty, in words the user can act on. */
export const REMOTE_REFUSALS: Record<InjectionRefusal, string> = {
  empty: "there was nothing to send",
  "permission-prompt": "the terminal is waiting on a permission prompt",
  modal: "a dialog in the terminal is waiting to be dismissed",
  "unrecognised-screen": "the terminal is showing something unrecognised",
  "command-prefix":
    "a message starting with /, ! or # drives the terminal's own menus instead of being sent",
};

// ------------------------------------------------------------ outbound text

/** Ctrl-U. Every write is verified against the screen before anything is typed. */
export const COMPOSER_CLEAR = "\x15";

/**
 * What the TUI's composer is holding, or `null` when no composer is on screen.
 *
 * An empty composer renders as a bare `❯`; text after it is content. Measured
 * from captures: idle and mid-turn screens show `❯` alone, and the screen 25s
 * after an ESC shows `❯ Write a very long essay about…` — the CLI restores the
 * interrupted prompt there. Nothing paints placeholder text into it, so the
 * remainder after the marker is real content rather than a hint.
 */
export function composerHeld(lines: readonly string[]): string | null {
  for (const line of lines) {
    const trimmed = line.trim();
    if (trimmed !== "❯" && !trimmed.startsWith("❯ ")) continue;
    const held = trimmed.slice(1).trim();
    return held.length > 0 ? held : null;
  }
  return null;
}

export type SendGate =
  | { kind: "send" }
  /** Clear the composer first, then re-check. Never typed blind. */
  | { kind: "clear"; held: string }
  | { kind: "refuse"; reason: string };

/**
 * Whether a composer turn may be typed into the pty.
 *
 * `planInjection` answers "is the screen injectable", and its rule — composer
 * idle, no modal — is necessary but not sufficient. **An idle composer holding
 * restored text passes it.** Measured: after an ESC, injecting `say OK`
 * produced one user message reading
 * `…Think carefully first.say OK`, with no separator, because bracketed paste
 * appends and the CR submits the concatenation. The assistant happened to ignore
 * the prefix, which is luck rather than safety.
 *
 * So a held composer is its own outcome: clear it, then look again. Nothing is
 * typed on the strength of the clear having probably worked.
 */
export function remoteSendGate(
  screen: PromptScreen | null,
  plan: InjectionPlan | null,
  terminalOpen = false,
): SendGate {
  // The user is typing into the TUI themselves. Injecting now would interleave
  // with their keystrokes in the same composer.
  if (terminalOpen) {
    return {
      kind: "refuse",
      reason: "the terminal is open and you are typing into it directly",
    };
  }
  if (!screen || !plan) {
    return { kind: "refuse", reason: "the terminal has not painted yet" };
  }
  if (!plan.allowed) {
    return { kind: "refuse", reason: REMOTE_REFUSALS[plan.reason] };
  }
  const held = composerHeld(screen.lines);
  if (held) return { kind: "clear", held };
  return { kind: "send" };
}

// ------------------------------------------------------------ pty fan-out

export type PtyFanout = {
  /** One decoded chunk of pty output, to the parser and every viewer. */
  dispatch: (chunk: string) => void;
  /** Adds a viewer. The returned function removes that viewer and only it. */
  attach: (read: (chunk: string) => void) => () => void;
  /** Viewers only. The parser is not one of them. */
  viewerCount: () => number;
};

/**
 * One subscription, many readers.
 *
 * `subscribePty` keeps a **single** handler per pty id, so a terminal view cannot
 * subscribe for itself: doing so would replace the screen parser's handler, and
 * its unsubscribe — guarded on `dataHandlers.get(id) === onData` — would then
 * delete the view's own entry and leave the pty **watched by nobody**, silently
 * and with no error anywhere.
 *
 * So the parser is passed in here rather than registered, which makes it
 * structurally undetachable: `attach` returns a remover closed over one viewer,
 * and there is no path by which any caller can remove the parser. That is the
 * guarantee, not a convention to be careful about.
 */
export function createPtyFanout(parser: (chunk: string) => void): PtyFanout {
  const viewers = new Set<(chunk: string) => void>();
  return {
    dispatch: (chunk) => {
      // The parser first, so a viewer that throws cannot cost it the chunk.
      parser(chunk);
      for (const read of [...viewers]) {
        try {
          read(chunk);
        } catch {
          // A disposed terminal throws on write. The pty subscription must
          // survive that, and the parser has already had the chunk.
        }
      }
    },
    attach: (read) => {
      viewers.add(read);
      return () => {
        viewers.delete(read);
      };
    },
    viewerCount: () => viewers.size,
  };
}

/**
 * How to describe a send's fate, when the screen may not know it.
 *
 * `kind` answers *can I inject*; `turn` answers *will it queue*. They were one
 * field's job and they are not the same question — during streaming the screen
 * reads `idle` with `turn: "unknown"`, so a send is allowed while whether the
 * TUI queues it is genuinely unknowable from a frame. Nothing is lost either
 * way: a mid-turn send was measured to queue cleanly. So the only wrong answer
 * here is a confident one.
 */
export function queuedNotice(queued: boolean | "unknown"): string | null {
  if (queued === "unknown") {
    return "Sent — it may be queued in the terminal until the current turn ends; the screen cannot tell while a turn is streaming";
  }
  return queued ? "Queued in the terminal until the current turn ends" : null;
}

export type InjectOutcome =
  | { kind: "sent"; queued: boolean | "unknown" }
  | { kind: "refused"; reason: string };

/** The impure edges of typing into a TUI, so the sequence can be asserted. */
export type InjectPorts = {
  write: (bytes: string) => Promise<void>;
  screen: () => PromptScreen | null;
  /** One poll interval. Resolves when the screen may have repainted. */
  settle: () => Promise<void>;
  /** Polls before giving up on a clear. */
  attempts?: number;
};

/**
 * Type a message into the pty, clearing a restored prompt out of the way first.
 *
 * The clear keystroke is written and then **checked**, never trusted: after an
 * ESC the CLI restores the interrupted prompt into its composer and bracketed
 * paste appends to it, so a send that assumed the clear worked would submit the
 * two welded together — measured as one message reading
 * `…Think carefully first.say OK`. A composer that will not come back empty
 * therefore ends in a refusal. Not knowing what is in the composer stops a send;
 * it never permits one.
 */
export async function injectRemoteText(
  text: string,
  terminalOpen: boolean,
  ports: InjectPorts,
): Promise<InjectOutcome> {
  const planFor = (screen: PromptScreen | null) =>
    screen ? planInjection(screen, text) : null;
  const gate = remoteSendGate(
    ports.screen(),
    planFor(ports.screen()),
    terminalOpen,
  );
  if (gate.kind === "refuse") return { kind: "refused", reason: gate.reason };

  if (gate.kind === "clear") {
    await ports.write(COMPOSER_CLEAR);
    let cleared = false;
    for (let attempt = 0; attempt < (ports.attempts ?? 20); attempt += 1) {
      await ports.settle();
      const lines = ports.screen()?.lines;
      if (lines && !composerHeld(lines)) {
        cleared = true;
        break;
      }
    }
    if (!cleared) {
      return {
        kind: "refused",
        reason: `the terminal's composer still holds "${gate.held}", which a send would be joined onto`,
      };
    }
  }

  const plan = planFor(ports.screen());
  if (!plan?.allowed) {
    return {
      kind: "refused",
      reason: plan
        ? REMOTE_REFUSALS[plan.reason]
        : "the terminal stopped painting while it was cleared",
    };
  }
  await ports.write(plan.bytes);
  return { kind: "sent", queued: plan.queued };
}

// ------------------------------------------------------- raising an approval

/**
 * Consecutive frames that must agree on the option set before a prompt is shown.
 *
 * 1..n numbering with exactly one cursor proves the run is **contiguous**, not
 * that it is **complete**: a box painted halfway gives `1..2 of 3`, which is
 * contiguous, singly-selected, and parses as a finished two-option prompt.
 * On the captured prompt the options are `Yes` / `Yes, and don't ask again` /
 * `No`, so a half-painted frame is an approval **with no denial on it** — the
 * user is offered Yes or Yes and the way out is not on screen. Re-reading every
 * chunk corrects the frame, but not before someone can answer the one in front
 * of them. A settled prompt costs one extra chunk; a half-painted one is never
 * raised.
 */
export const APPROVAL_STABLE_FRAMES = 2;

/**
 * Distinct option sets in a row before the screen is treated as unreadable.
 *
 * A screen that keeps repainting is not a prompt being read, it is a prompt we
 * cannot read — which is the raw-pty case. Refusing to raise is the safe
 * direction, but it must not be silent, so it ends with the terminal in front of
 * the user rather than with nothing.
 */
export const APPROVAL_CHURN_LIMIT = 3;

export type ApprovalShown =
  { kind: "question"; requestId: number } | { kind: "raw" } | null;

export type ApprovalProgress = {
  shown: ApprovalShown;
  /** The option set last parsed, and how many frames running it has held. */
  signature: string | null;
  agreed: number;
  /** Distinct option sets seen in a row without one settling. */
  churn: number;
};

export type ApprovalEffect =
  | { kind: "ask"; requestId: number; question: UserQuestion }
  | { kind: "resolve"; requestId: number; decision: "cancelled" }
  | { kind: "openTerminal" }
  | { kind: "surface"; lines: readonly string[]; reason: string };

export type ApprovalStep = {
  progress: ApprovalProgress;
  effects: ApprovalEffect[];
};

export function emptyApprovalProgress(): ApprovalProgress {
  return { shown: null, signature: null, agreed: 0, churn: 0 };
}

/**
 * What a prompt is, for the purpose of deciding it is the same prompt.
 *
 * The question and its option labels, in order. Three deliberate exclusions:
 *
 * The **selected index** is out, and by construction rather than by choice here:
 * `questionFromPrompt` keeps only each option's id and label, so selection never
 * reaches this function at all. That is the right outcome — a cursor moving is
 * navigation within one prompt, and counting it would mean arrow keys suppressed
 * the raise — but it is enforced by the view's shape, so no test here can break
 * it and none pretends to.
 *
 * The **deny footer** is out, because after #38 its absence is legitimate — a
 * real `WebFetch` prompt prints no footer at all — so a missing one cannot
 * distinguish a partial paint from a complete one, and a half-painted *footer*
 * is not a safety problem the way a half-painted option list is.
 *
 * **Counts** need no separate mention: fewer labels is a different signature,
 * which is exactly the case this exists to catch.
 */
function approvalSignature(question: UserQuestion): string {
  return JSON.stringify([
    question.prompt,
    question.options.map((option) => option.label),
  ]);
}

/**
 * Raise, retire or re-raise the approval a session is waiting on.
 *
 * A function over `(what is shown, what the screen says)` so every path is a row
 * in a table rather than an emergent property of the order the branches happen
 * to be written in. The caller performs the effects; deciding which ones is all
 * that happens here.
 */
export function remoteApprovalTransition(
  progress: ApprovalProgress,
  view: RemoteApprovalView,
  nextRequestId: number,
): ApprovalStep {
  const cleared = emptyApprovalProgress();

  if (view.kind === "none") {
    // Answered on the phone or in the TUI while MonoCode was showing it.
    // `cancelled`, not `skipped`: nobody declined it here.
    const effects: ApprovalEffect[] =
      progress.shown?.kind === "question"
        ? [
            {
              kind: "resolve",
              requestId: progress.shown.requestId,
              decision: "cancelled",
            },
          ]
        : [];
    return { progress: cleared, effects };
  }

  if (view.kind === "raw") {
    if (progress.shown?.kind === "raw") return { progress, effects: [] };
    return {
      progress: { ...cleared, shown: { kind: "raw" } },
      effects: [
        { kind: "openTerminal" },
        { kind: "surface", lines: view.lines, reason: view.reason },
      ],
    };
  }

  // Already in front of the user. Re-raising would allocate a second request id
  // for one prompt.
  if (progress.shown?.kind === "question") return { progress, effects: [] };

  const signature = approvalSignature(view.question);
  const fresh = signature !== progress.signature;
  const churn = fresh ? progress.churn + 1 : progress.churn;
  // A first sighting counts as one agreeing frame, not as a special case: if it
  // returned early the threshold could never be read on this path, and
  // `APPROVAL_STABLE_FRAMES = 1` would behave exactly like `2`.
  const agreed = fresh ? 1 : progress.agreed + 1;
  if (fresh) {
    if (churn >= APPROVAL_CHURN_LIMIT) {
      // Repainting rather than settling, so this is a screen we cannot read.
      return {
        progress: { ...cleared, shown: { kind: "raw" } },
        effects: [
          { kind: "openTerminal" },
          {
            kind: "surface",
            lines: view.lines,
            reason: "the options kept changing",
          },
        ],
      };
    }
  }

  if (agreed < APPROVAL_STABLE_FRAMES) {
    return {
      progress: { shown: progress.shown, signature, agreed, churn },
      effects: [],
    };
  }
  return {
    progress: {
      shown: { kind: "question", requestId: nextRequestId },
      signature,
      agreed,
      churn: 0,
    },
    effects: [
      { kind: "ask", requestId: nextRequestId, question: view.question },
    ],
  };
}

// ------------------------------------------------------------- turn liveness

/**
 * What to tell `resolveTurnFromScreen` about a turn.
 *
 * Two readings, in priority order. A screen that says the turn is **over** is
 * definite — `COMPLETED` matched a real `for 2s` marker — so it is passed as the
 * plain boolean and nothing else is considered. Otherwise the only evidence an
 * interrupt leaves is negative: the pty stops producing output, measured at
 * 800ms worst case across a live turn against indefinite afterwards, and the CLI
 * restores the interrupted prompt into the composer.
 *
 * A pty that has produced **nothing at all** reports no signal. Quiet since a
 * moment we never observed is not evidence of silence, and an absence of
 * evidence must never end a turn.
 */
export function turnSignal(
  screen: PromptScreen | null,
  lastChunkAt: number | null,
  now: number,
): boolean | TurnLiveness {
  if (!screen) return false;
  if (screen.turn === "ended") return true;
  if (lastChunkAt === null) return false;
  return {
    quietForMs: Math.max(0, now - lastChunkAt),
    composerHeld: composerHeld(screen.lines) !== null,
  };
}

/**
 * Run `open` for each id, letting one failure cost only its own session.
 *
 * A remote-control failure has to degrade that conversation, not the window. The
 * loop this replaces called straight into the opener, so anything thrown for one
 * session abandoned every session after it — and in an effect, took the React
 * tree with it.
 */
export function forEachSafely(
  ids: readonly string[],
  open: (id: string) => void,
  onError: (id: string, error: unknown) => void,
): void {
  for (const id of ids) {
    try {
      open(id);
    } catch (error) {
      onError(id, error);
    }
  }
}

/**
 * Run one hand-over with its session claimed for the whole of it.
 *
 * The claim is what stops `all` mode starting a second hand-over for a session
 * whose first one has not finished: the entry that marks it done is written many
 * awaits in, and until then nothing else says this one is being taken. Releasing
 * it was previously spelled out on the two paths someone thought of, so a throw
 * anywhere else stranded the id for the life of the window — and `shouldAutoOpen`
 * reads that set as `open`, so the session could never be handed over again.
 *
 * Silently, which is the worse half. The caller is async, so the throw is a
 * rejected promise; `forEachSafely` wraps the *call*, not the settlement, and
 * never sees it. So the error is reported here rather than left to a handler that
 * structurally cannot receive it.
 *
 * `finally` rather than two call sites to remember, because "every exit releases
 * the claim" is the whole point and a rule kept by hand is a rule waiting to be
 * missed once.
 */
export async function withClaim(
  claimed: Set<string>,
  id: string,
  run: () => Promise<void>,
  onError: (error: unknown) => void,
): Promise<void> {
  if (claimed.has(id)) return;
  claimed.add(id);
  try {
    await run();
  } catch (error) {
    onError(error);
  } finally {
    claimed.delete(id);
  }
}

/**
 * Which sessions `all` mode should hand over now.
 *
 * Separated from the loop so the choice stays a function of the sessions and
 * their state, and so a caller cannot accidentally make "already opening" part
 * of the iteration rather than part of the decision — which is what let one
 * session be opened twice, and then unboundedly.
 */
export function autoOpenTargets(
  sessions: readonly Session[],
  stateFor: (session: Session) => AutoOpenState,
): string[] {
  return sessions
    .filter((session) => shouldAutoOpen(session, stateFor(session)))
    .map((session) => session.id);
}
