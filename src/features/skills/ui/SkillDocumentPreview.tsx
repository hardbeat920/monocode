import { MarkdownDocumentPreview } from "../../sessions/ui/MarkdownDocumentPreview";

/** Keep the skill's YAML header readable without interpreting it as Markdown. */
export function SkillDocumentPreview({ text }: { text: string }) {
  return <MarkdownDocumentPreview text={text} metadataLabel="Skill metadata" />;
}
