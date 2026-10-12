import { CONTINUE_PROMPT } from "../../sessions/model/inFlight";
import type { Session } from "../../sessions/model/session";
import { sameProviderAccountId } from "../../providers/model/providerAccounts";
import { resumeUsageLimitedSession } from "../../sessions/model/usageLimit";
import { enqueueMonoMessage } from "./monoMessaging";

/** Retry the existing outbox in order, or continue the stopped work once. */
export function resumeMonoUsageLimit(session: Session): Session {
  if (!session.usageLimit || session.busy) return session;
  const resumed = resumeUsageLimitedSession(session);
  return resumed.queuedMessages?.length
    ? resumed
    : enqueueMonoMessage(resumed, {
        id: crypto.randomUUID(),
        text: CONTINUE_PROMPT,
        attachments: [],
      });
}

/** Account-owned threads need a fresh connection with a transcript handoff. */
export function switchMonoUsageLimitAccount(
  session: Session,
  accountId: string,
): Session {
  if (
    !session.usageLimit ||
    session.busy ||
    sameProviderAccountId(session.providerAccountId, accountId)
  )
    return session;
  return resumeMonoUsageLimit({
    ...session,
    providerAccountId: accountId,
    providerSessionId: undefined,
    pendingSwitch: {
      ...(session.pendingSwitch ?? {
        from: session.harness,
        fromModel: session.model,
        fromSettings: session.modelSettings,
        fromProviderSessionId: session.providerSessionId,
        fromProviderAccountId: session.providerAccountId,
      }),
      skipOutgoingRecap: true,
    },
  });
}
