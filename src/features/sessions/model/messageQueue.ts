import { isPreparingHandoff } from "./handoff";
import { mergeModelSettings, resolveModel } from "./models";
import { runningProviderSelection } from "./providerContext";
import type { ModelTarget, QueuedMessage, Session } from "./session";

export function queuedHead(session: Session): QueuedMessage | undefined {
  return session.queuedMessages?.[0];
}

/** Hold auto-dispatch only while the item about to send is being edited. */
export function isEditingQueuedHead(session: Session): boolean {
  const head = queuedHead(session);
  return Boolean(head && session.editingQueuedMessageId === head.id);
}

export function dequeueQueuedMessage(
  session: Session,
  messageId: string,
): Session {
  const queuedMessages = (session.queuedMessages ?? []).filter(
    (message) => message.id !== messageId,
  );
  return {
    ...session,
    queuedMessages: queuedMessages.length > 0 ? queuedMessages : undefined,
    queueStatus: queuedMessages.length > 0 ? session.queueStatus : undefined,
    editingQueuedMessageId:
      session.editingQueuedMessageId === messageId
        ? undefined
        : session.editingQueuedMessageId,
  };
}

/**
 * True when the idle session can send its queued head as a new turn.
 * Busy / paused / resuming / usage-limited / preparing-handoff /
 * editing-the-head all wait.
 */
export function canDispatchQueuedHead(session: Session): boolean {
  if (session.busy) return false;
  if (session.worktreePreparing || session.worktreeRemoved) return false;
  if (session.usageLimit) return false;
  if (session.providerContext?.delivery?.needsInspection) return false;
  if (session.queueStatus === "paused" || session.queueStatus === "resuming") {
    return false;
  }
  const head = queuedHead(session);
  if (!head) return false;
  if (isEditingQueuedHead(session)) return false;
  if (isPreparingHandoff(session)) return false;
  return true;
}

function sameSteeringSelection(
  saved: ModelTarget,
  active: ModelTarget,
): boolean {
  if (saved.harness !== active.harness || saved.model !== active.model)
    return false;
  const model = resolveModel(saved.harness, saved.model);
  const savedSettings = {
    ...saved.modelSettings,
    ...mergeModelSettings(model, saved.modelSettings),
  };
  const activeSettings = {
    ...active.modelSettings,
    ...mergeModelSettings(model, active.modelSettings),
  };
  const keys = new Set([
    ...Object.keys(savedSettings),
    ...Object.keys(activeSettings),
  ]);
  return [...keys].every((key) => savedSettings[key] === activeSettings[key]);
}

/** Monos deliver one waiting follow-up at a time after the provider is ready. */
export function canSteerQueuedHead(session: Session): boolean {
  const head = queuedHead(session);
  return (
    !!head &&
    !head.monoSessionCompletion &&
    !!session.busy &&
    !!session.turnReady &&
    !session.worktreePreparing &&
    !session.worktreeRemoved &&
    !session.pendingSwitch &&
    !session.usageLimit &&
    !(
      session.pendingQuestion && session.pendingQuestion.autoResolveAt == null
    ) &&
    session.queueStatus !== "paused" &&
    session.queueStatus !== "resuming" &&
    !isEditingQueuedHead(session) &&
    !isPreparingHandoff(session) &&
    !session.providerContext?.delivery?.needsInspection &&
    head.intent !== "plan" &&
    head.intent !== "orchestrate"
  );
}

/** Resolve a queued row for auto-dispatch (head, idle) or an explicit Steer. */
export function queuedMessageForSubmit(
  session: Session,
  messageId: string,
  mode: "dispatch" | "steer",
  activeSelection?: ModelTarget,
): QueuedMessage | undefined {
  if (session.providerContext?.delivery?.needsInspection) return undefined;
  const message = session.queuedMessages?.find(
    (entry) => entry.id === messageId,
  );
  if (!message) return undefined;
  if (mode === "steer") {
    if (message.monoSessionCompletion) return undefined;
    if (
      session.busy &&
      message.selection &&
      !sameSteeringSelection(
        message.selection,
        activeSelection ?? runningProviderSelection(session),
      )
    )
      return undefined;
    return message;
  }
  if (queuedHead(session)?.id !== messageId) return undefined;
  if (!canDispatchQueuedHead(session)) return undefined;
  return message;
}
