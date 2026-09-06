import { applyFileMentionsToTurn } from "./fileMentions";
import { applyNotesToTurn } from "./notes";
import {
  applySkillsToTurn,
  warmNativeSkills,
  isNativeCommandPrompt,
  type SkillCatalogContext,
} from "./skills";
import { nativeCommandPrompt } from "./harness/nativeCommands";

export async function preparePrompt(
  text: string,
  context: SkillCatalogContext,
): Promise<string> {
  warmNativeSkills(context);
  if (isNativeCommandPrompt(text, context.harness))
    return nativeCommandPrompt(context.harness, text);
  const withFiles = await applyFileMentionsToTurn(text, context.cwd);
  const withNotes = await applyNotesToTurn(withFiles);
  return applySkillsToTurn(withNotes, context);
}
