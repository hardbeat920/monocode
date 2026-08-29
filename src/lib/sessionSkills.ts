import {
  sessionWorkCwd,
  type HarnessId,
} from "./session";
import type { SkillCatalogContext } from "./skills";

type SkillWarmupSession = {
  harness: HarnessId;
  cwd: string;
  worktreeCwd?: string;
};

export function piSkillContextForSession(
  session: SkillWarmupSession,
): SkillCatalogContext | null {
  if (session.harness !== "pi") return null;
  return { harness: "pi", cwd: sessionWorkCwd(session) };
}
