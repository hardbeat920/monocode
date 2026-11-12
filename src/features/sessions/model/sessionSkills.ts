import { sessionWorkCwd, type HarnessId } from "./session";
import { hasNativeCommands, type SkillCatalogContext } from "../../skills/model/skills";
import {
  selectedProviderAccountId,
  supportsProviderAccounts,
} from "../../providers/model/providerAccounts";

type SkillWarmupSession = {
  id?: string;
  harness: HarnessId;
  cwd: string;
  worktreeCwd?: string;
  providerAccountId?: string;
};

/** The account a session's next turn runs under, as the send path resolves it. */
export function sessionSkillAccountId(session: {
  harness: HarnessId;
  cwd: string;
  providerAccountId?: string;
}): string | undefined {
  if (!supportsProviderAccounts(session.harness)) return undefined;
  return (
    session.providerAccountId ??
    selectedProviderAccountId(session.harness, session.cwd)
  );
}

export function nativeSkillContextForSession(
  session: SkillWarmupSession,
): SkillCatalogContext | null {
  if (!hasNativeCommands(session.harness)) return null;
  const accountId = sessionSkillAccountId(session);
  return {
    harness: session.harness,
    cwd: sessionWorkCwd(session),
    ...(session.id ? { sessionId: session.id } : {}),
    ...(accountId ? { accountId } : {}),
  };
}
