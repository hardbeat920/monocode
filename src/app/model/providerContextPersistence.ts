import {
  shouldPersistSession,
  upsertSession,
} from "../../features/sessions/data/sessionStore";
import type { Session } from "../../features/sessions/model/session";

export async function saveProviderContextSession(
  session: Session | undefined,
  failureMessage: string,
): Promise<void> {
  if (
    !session ||
    (shouldPersistSession(session) && !(await upsertSession(session)))
  ) {
    throw new Error(failureMessage);
  }
}
