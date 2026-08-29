import { applyFileMentionsToTurn } from "./fileMentions";
import { applyNotesToTurn } from "./notes";
import {
  applySkillsToTurn,
  warmPiSkills,
  type SkillCatalogContext,
} from "./skills";

export async function preparePrompt(
  text: string,
  context: SkillCatalogContext,
): Promise<string> {
  warmPiSkills(context);
  const withFiles = await applyFileMentionsToTurn(text, context.cwd);
  const withNotes = await applyNotesToTurn(withFiles);
  return applySkillsToTurn(withNotes, context);
}
