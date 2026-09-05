import {
  formatReleaseDate,
  presentReleaseNotes,
  releaseNotesTitle,
} from "../lib/releaseNotes";
import { AgentMarkdown } from "../surfaces/AgentMarkdown";
import { Modal } from "./Modal";
import { t } from "../lib/i18n";

type Props = {
  version: string;
  onClose: () => void;
};

export function WhatsNewBody({ version }: { version: string }) {
  const notes = presentReleaseNotes(version);
  const title = releaseNotesTitle(version);

  return (
    <article aria-label={title} className="px-5 py-4">
      {notes?.markdown ? (
        <AgentMarkdown
          className="whats-new-md"
          text={notes.markdown}
          streaming={false}
        />
      ) : (
        <p className="text-[13px] text-content/60">
          {t("Release notes for this version are not available in this build.")}
        </p>
      )}
    </article>
  );
}

export function WhatsNewDialog({ version, onClose }: Props) {
  const notes = presentReleaseNotes(version);
  const date = notes?.date ? formatReleaseDate(notes.date) : null;

  return (
    <Modal
      onClose={onClose}
      title={t("What's new")}
      description={`${"MonoCode"} ${version}${date ? ` · ${date}` : ""}`}
      size="md"
      className="h-[min(72vh,640px)]"
    >
      <WhatsNewBody version={version} />
    </Modal>
  );
}
