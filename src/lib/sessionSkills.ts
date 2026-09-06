import { sessionWorkCwd, type HarnessId } from "./session";
import { hasNativeCommands, type SkillCatalogContext } from "./skills";

type SkillWarmupSession = {
  id?: string;
  harness: HarnessId;
  cwd: string;
  worktreeCwd?: string;
};

export function nativeSkillContextForSession(
  session: SkillWarmupSession,
): SkillCatalogContext | null {
  if (!hasNativeCommands(session.harness)) return null;
  return {
    harness: session.harness,
    cwd: sessionWorkCwd(session),
    ...(session.id ? { sessionId: session.id } : {}),
  };
}
