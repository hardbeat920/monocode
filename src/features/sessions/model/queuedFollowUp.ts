import { TurnNotReadyError } from "../../../integrations/harness/core/types";
import { canSteerQueuedHead, queuedHead } from "./messageQueue";
import type { QueuedMessage, Session } from "./session";

/** Keep the pending message until the provider accepts it. Preparation is part of delivery. */
export async function deliverQueuedFollowUp<T>(options: {
  message: QueuedMessage;
  current: () => Session | undefined;
  isCurrentTurn: () => boolean;
  prepare: () => Promise<T>;
  steer: (prepared: T) => Promise<void>;
  delivered: () => void;
  deferred: () => void;
  failed: (error: string) => void;
}): Promise<boolean> {
  const canDeliver = () => {
    const session = options.current();
    return (
      options.isCurrentTurn() &&
      !!session &&
      queuedHead(session) === options.message &&
      canSteerQueuedHead(session)
    );
  };
  if (!canDeliver()) return false;
  try {
    const prepared = await options.prepare();
    // An edit, cancellation, provider switch or stop during preparation must win.
    if (!canDeliver()) return false;
    await options.steer(prepared);
  } catch (error) {
    if (!options.isCurrentTurn()) return false;
    if (error instanceof TurnNotReadyError) options.deferred();
    else options.failed(error instanceof Error ? error.message : String(error));
    return false;
  }
  options.delivered();
  return true;
}
