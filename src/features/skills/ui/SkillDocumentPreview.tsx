import { useTranslation } from "../../i18n/model/i18n";
import { MarkdownDocumentPreview } from "../../sessions/ui/MarkdownDocumentPreview";

/** Keep the skill's YAML header readable without interpreting it as Markdown. */
export function SkillDocumentPreview({ text }: { text: string }) {
  const { t } = useTranslation();
  return (
    <MarkdownDocumentPreview
      text={text}
      metadataLabel={t("skills.metadataLabel", "Skill metadata")}
    />
  );
}
