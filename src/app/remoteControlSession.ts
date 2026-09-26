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
): RemoteControlTarget {
  return {
    harness: session.harness,
    providerSessionId: session.providerSessionId,
    active,
  };
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
export function shouldAutoOpen(
  session: Session,
  state: AutoOpenState,
): boolean {
  if (state.mode !== "all") return false;
  if (state.open || state.dismissed) return false;
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
  | { kind: "question"; question: UserQuestion; cancel?: string }
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
      ...(screen.prompt.deny ? { cancel: screen.prompt.deny.label } : {}),
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
