import type {
  AppSendResult,
  WhenBusy,
} from "../../features/agent-app/model/agentApp";
import type { ControlOutcome } from "../../features/orchestration/model/orchestration";
import { isPreparingHandoff } from "../../features/sessions/model/handoff";
import { consumeOperatorCommand } from "../../features/sessions/model/operatorCommand";
import type {
  AppDelivery,
  HarnessId,
  QueuedMessage,
  Session,
} from "../../features/sessions/model/session";
import { appendSteerUser } from "../../integrations/harness/core/apply";

export const SESSION_BUSY_ERROR = "Session is busy; try again when it finishes";
export const TURN_FINISHED_ERROR =
  "The running turn just finished; send the message again";
export const QUEUED_FOLLOW_UP_REMOVED =
  "The queued follow-up was removed before it was sent.";
export const QUEUED_FOLLOW_UP_ARCHIVED =
  "The session was archived before the queued follow-up was sent.";

/** An earlier delivery of the same CLI request. Throws when a sent or steered
 *  request is reused with another prompt. A queued follow-up matches by ID
 *  alone, because the user can edit its text. */
export function priorAppSend(
  session: Pick<Session, "blocks" | "queuedMessages">,
  requestId: string,
  prompt: string,
): AppSendResult | undefined {
  const queued = session.queuedMessages?.find(
    (message) => message.appRequestId === requestId,
  );
  if (queued)
    return {
      alreadySubmitted: true,
      delivery: "queued",
      queuedMessageId: queued.id,
    };
  const previous = session.blocks.find(
    (block) => block.appRequestId === requestId,
  );
  if (!previous) return undefined;
  if (previous.appDelivery?.via === "queued")
    return {
      alreadySubmitted: true,
      delivery: "queued",
      queuedMessageId: previous.appDelivery.queuedMessageId,
    };
  if (previous.text !== prompt)
    throw new Error("Request ID was already used with another prompt");
  if (previous.draft) throw new Error("Request ID belongs to an unsent draft");
  return {
    alreadySubmitted: true,
    delivery: previous.appDelivery?.via === "steered" ? "steered" : "sent",
  };
}

/** Why the session state does not allow a CLI steer now, or undefined. */
export function steerBlockReason(
  session: Session,
  prompt: string,
): string | undefined {
  if (session.worktreePreparing)
    return 'The session is preparing its worktree and cannot be steered yet; use whenBusy "followUp"';
  if (session.pendingSwitch || isPreparingHandoff(session))
    return 'The session is switching providers and cannot be steered; use whenBusy "followUp"';
  if (consumeOperatorCommand(prompt).matched)
    return '/operator starts a new turn and cannot steer the running one; use whenBusy "followUp"';
  return undefined;
}

export type AppSendPlan = "send" | "steer" | "queue";
/** Decide the delivery. Throws a user-facing Error when the send must fail. */
export function planAppSend(input: {
  harness: HarnessId;
  busy: boolean;
  whenBusy?: WhenBusy;
  /** isLiveHarness(harness) && canSteerHarness(harness), from the caller. */
  steerable: boolean;
  /** steerBlockReason(session, prompt), from the caller. */
  steerBlocked?: string;
  /** isMonoSession(id), from the caller. */
  mono: boolean;
}): AppSendPlan {
  if (!input.busy) return "send";
  if (input.mono)
    throw new Error(
      input.whenBusy === undefined
        ? SESSION_BUSY_ERROR
        : "A Mono's own conversation does not accept whenBusy; try again when it finishes",
    );
  if (input.whenBusy === undefined) throw new Error(SESSION_BUSY_ERROR);
  if (input.whenBusy === "followUp") return "queue";
  // Hermes may queue a "steer" as a follow-up, and its acknowledgement can time
  // out after delivery, so the CLI could not report "steered" truthfully.
  if (!input.steerable || input.harness === "hermes")
    throw new Error(
      `${input.harness} cannot be steered mid-turn; use whenBusy "followUp" or wait for the turn to finish`,
    );
  if (input.steerBlocked) throw new Error(input.steerBlocked);
  return "steer";
}

/** Append the steer's user block, tagged so a retry reports "steered".
 *  Keeps `busy`: a steer never starts a turn. */
export function recordAppSteer(
  session: Session,
  prompt: string,
  requestId: string,
): Session {
  return {
    ...session,
    blocks: appendSteerUser(session, prompt, [], {
      appRequestId: requestId,
      appDelivery: { via: "steered" },
    }).blocks,
  };
}

/** Remove the steer block of `requestId` after the harness rejected the steer. */
export function forgetAppSteer(session: Session, requestId: string): Session {
  const blocks = session.blocks.filter(
    (block) =>
      block.appRequestId !== requestId || block.appDelivery?.via !== "steered",
  );
  return blocks.length === session.blocks.length
    ? session
    : { ...session, blocks };
}

/** Add one CLI follow-up to the visible queue. */
export function queueAppFollowUp(
  session: Session,
  item: { id: string; prompt: string; requestId: string },
): Session {
  return {
    ...session,
    queuedMessages: [
      ...(session.queuedMessages ?? []),
      {
        id: item.id,
        text: item.prompt,
        attachments: [],
        appRequestId: item.requestId,
      },
    ],
    queueStatus: session.queueStatus === "paused" ? "paused" : "active",
  };
}

/** Block fields for a user turn that sends queued item `message`. */
export function queuedAppDelivery(message: QueuedMessage | undefined): {
  appRequestId?: string;
  appDelivery?: AppDelivery;
} {
  if (!message?.appRequestId) return {};
  return {
    appRequestId: message.appRequestId,
    appDelivery: { via: "queued", queuedMessageId: message.id },
  };
}

type AcceptedWatcher = CompletionWatcher & {
  accept: () => void;
  discard: () => void;
};

/** Check that turn `gen` still runs, then record, await the harness, and undo
 *  on failure. `watcher` follows turn `gen` once the harness accepts. */
export async function deliverAppSteer(steps: {
  watchers: TurnWatchers;
  sessionId: string;
  gen: number;
  watcher?: AcceptedWatcher;
  record: () => void;
  steer: () => Promise<void>;
  forget: () => void;
}): Promise<AppSendResult> {
  const { watchers, sessionId, gen, watcher } = steps;
  if (watchers.settled(sessionId, gen)) {
    watcher?.discard();
    throw new Error(TURN_FINISHED_ERROR);
  }
  if (watcher) watchers.attach(sessionId, gen, watcher);
  try {
    steps.record();
    await steps.steer();
  } catch (error: unknown) {
    steps.forget();
    if (watcher) {
      watchers.detach(sessionId, gen, watcher);
      watcher.discard();
    }
    throw error;
  }
  watcher?.accept();
  return { alreadySubmitted: false, delivery: "steered" };
}

/** Steer a user message into running turn `gen`. A queued item's CLI watcher
 *  follows that turn, or gets the steer's failure. Returns false and changes
 *  nothing when that turn already settled, so the item stays queued with its
 *  watcher. `fail` reports a rejected steer to the user. */
export function steerRunningTurn(steps: {
  watchers: TurnWatchers;
  sessionId: string;
  gen: number;
  queued?: { id: string; watcher?: CompletionWatcher };
  record: () => void;
  steer: () => Promise<void>;
  fallbackError: string;
  fail: (message: string) => void;
}): boolean {
  const { watchers, sessionId, gen, queued } = steps;
  const watcher = queued && (queued.watcher ?? watchers.take(queued.id));
  if (watcher && !watchers.attach(sessionId, gen, watcher)) {
    if (!queued.watcher) watchers.hold(queued.id, watcher);
    return false;
  }
  steps.record();
  void steps.steer().catch((error: unknown) => {
    const message =
      error instanceof Error ? error.message : steps.fallbackError;
    if (watcher && watchers.detach(sessionId, gen, watcher))
      watcher({ status: "failed", text: "", error: message });
    steps.fail(message);
  });
  return true;
}

export type CompletionWatcher = (outcome: ControlOutcome) => void;
export type TurnWatchers = {
  /** Attach to running turn `gen`. Returns false when that turn already settled. */
  attach(sessionId: string, gen: number, watcher: CompletionWatcher): boolean;
  /** Returns false when `watcher` was not attached, e.g. its turn settled. */
  detach(sessionId: string, gen: number, watcher: CompletionWatcher): boolean;
  /** True while at least one watcher is attached to turn `gen`. */
  watching(sessionId: string, gen: number): boolean;
  /** True once turn `gen`, or a newer turn of the session, has settled. */
  settled(sessionId: string, gen: number): boolean;
  /** Call and drop each watcher of turn `gen`. A later attach to this gen or
   *  an older gen returns false. */
  settle(sessionId: string, gen: number, outcome: ControlOutcome): void;
  /** Keep a watcher for queued item `messageId` until it is sent or removed. */
  hold(messageId: string, watcher: CompletionWatcher): void;
  /** Remove and return the watcher of queued item `messageId`. */
  take(messageId: string): CompletionWatcher | undefined;
  /** Queued item `messageId` will not be sent: call its watcher with
   *  { status: "cancelled", text: "", error } and drop it. */
  cancel(messageId: string, error?: string): void;
};

/** Completion watchers for CLI sends, keyed by turn or queued item. */
export function createTurnWatchers(): TurnWatchers {
  const turns = new Map<string, Map<number, Set<CompletionWatcher>>>();
  const settledGens = new Map<string, number>();
  const held = new Map<string, CompletionWatcher>();
  const take = (messageId: string) => {
    const watcher = held.get(messageId);
    held.delete(messageId);
    return watcher;
  };
  const settled = (sessionId: string, gen: number) =>
    gen <= (settledGens.get(sessionId) ?? -Infinity);
  return {
    attach(sessionId, gen, watcher) {
      if (settled(sessionId, gen)) return false;
      let gens = turns.get(sessionId);
      if (!gens) {
        gens = new Map<number, Set<CompletionWatcher>>();
        turns.set(sessionId, gens);
      }
      let watchers = gens.get(gen);
      if (!watchers) {
        watchers = new Set<CompletionWatcher>();
        gens.set(gen, watchers);
      }
      watchers.add(watcher);
      return true;
    },
    detach(sessionId, gen, watcher) {
      const gens = turns.get(sessionId);
      const watchers = gens?.get(gen);
      if (!gens || !watchers?.delete(watcher)) return false;
      if (watchers.size === 0) gens.delete(gen);
      if (gens.size === 0) turns.delete(sessionId);
      return true;
    },
    watching(sessionId, gen) {
      return (turns.get(sessionId)?.get(gen)?.size ?? 0) > 0;
    },
    settled,
    settle(sessionId, gen, outcome) {
      settledGens.set(
        sessionId,
        Math.max(gen, settledGens.get(sessionId) ?? gen),
      );
      const gens = turns.get(sessionId);
      const watchers = gens?.get(gen);
      if (!gens || !watchers) return;
      gens.delete(gen);
      if (gens.size === 0) turns.delete(sessionId);
      watchers.forEach((watcher) => watcher(outcome));
    },
    hold(messageId, watcher) {
      held.set(messageId, watcher);
    },
    take,
    cancel(messageId, error = QUEUED_FOLLOW_UP_REMOVED) {
      take(messageId)?.({ status: "cancelled", text: "", error });
    },
  };
}
