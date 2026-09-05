import { useLockOverscroll } from "../hooks/useLockOverscroll";
import { t } from "../lib/i18n";
import {
  releaseNotesMarkdown,
  type ReleaseNotesTabSource,
} from "../lib/releaseNotes";
import { AgentMarkdown } from "./AgentMarkdown";

export function ReleaseNotesSurface({
  source,
}: {
  source: ReleaseNotesTabSource;
}) {
  const lockOverscroll = useLockOverscroll<HTMLDivElement>();
  const markdown = releaseNotesMarkdown(source);

  return (
    <div
      ref={lockOverscroll}
      className="h-full overflow-y-auto overscroll-none"
    >
      <article
        aria-label={t("Release notes")}
        className="mx-auto w-full max-w-3xl px-8 py-10"
      >
        {markdown ? (
          <AgentMarkdown text={markdown} streaming={false} />
        ) : (
          <p className="text-[13px] text-content/60">
            {t("Release notes for this version are not available in this build.")}
          </p>
        )}
      </article>
    </div>
  );
}
