import { applyFileMentionsToTurn } from "../../files/model/fileMentions";
import { applyNotesToTurn } from "../../notes";
import {
  applySkillsToTurn,
  prepareNativeCommand,
  warmNativeSkills,
  isNativeCommandPrompt,
  type SkillCatalogContext,
} from "../../skills/model/skills";
import { nativeCommandPrompt } from "../../../integrations/harness/core/nativeCommands";

export async function preparePrompt(
  text: string,
  context: SkillCatalogContext,
  turn?: { effort?: string; steer?: boolean },
): Promise<string> {
  warmNativeSkills(context);
  // Runs alongside the rest; the turn must not start until it settles. A
  // steer can't wait on it: its turn may end meanwhile.
  const harnessReady = turn?.steer
    ? Promise.resolve()
    : prepareNativeCommand(text, context, turn?.effort);
  if (isNativeCommandPrompt(text, context.harness)) {
    await harnessReady;
    return nativeCommandPrompt(context.harness, text);
  }
  const withFiles = await applyFileMentionsToTurn(text, context.cwd);
  const withNotes = await applyNotesToTurn(withFiles);
  const prepared = await applySkillsToTurn(withNotes, context);
  await harnessReady;
  return prepared;
}
