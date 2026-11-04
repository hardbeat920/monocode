import { useEffect } from "react";
import { sameProviderAccountId } from "../../features/providers/model/providerAccounts";
import type { Session } from "../../features/sessions/model/session";

export function useProviderSignInRequestGuard(
  active: Pick<Session, "id" | "providerAccountId"> | null | undefined,
  request: { sessionId: string; providerAccountId?: string } | null,
  dismiss: (value: null) => void,
): void {
  useEffect(() => {
    if (
      request &&
      (active?.id !== request.sessionId ||
        !sameProviderAccountId(
          active?.providerAccountId,
          request.providerAccountId,
        ))
    ) {
      dismiss(null);
    }
  }, [active?.id, active?.providerAccountId, request, dismiss]);
}
