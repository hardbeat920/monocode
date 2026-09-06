import { X } from "./icons";
import { attachmentPreviewSrc } from "../lib/attachments";
import type { Attachment } from "../lib/session";
import { FileTypeIcon } from "./FileTypeIcon";
import { t } from "../lib/i18n";

type Props = {
  attachment: Attachment;
  onRemove?: () => void;
};

export function AttachmentChip({ attachment, onRemove }: Props) {
  const preview = attachmentPreviewSrc(attachment);
  const image = attachment.kind === "image" && preview;

  return (
    <div
      className={`group relative flex min-w-0 items-center gap-1.5 rounded-md ${
        image ? "" : "bg-content/10 py-0.5 pl-1 pr-1"
      }`}
      title={attachment.path ?? attachment.name}
    >
      {image ? (
        <img
          src={preview}
          alt=""
          className="size-9 shrink-0 rounded-lg object-cover"
        />
      ) : (
        <>
          <span className="grid size-5 shrink-0 place-items-center">
            <FileTypeIcon name={attachment.name} isDir={false} size={16} />
          </span>
          <span className="min-w-0 max-w-[140px] truncate text-[11px] leading-none text-content/80">
            {attachment.name}
          </span>
        </>
      )}
      {onRemove ? (
        <button
          type="button"
          title={t("Remove")}
          aria-label={t("Remove {0}", [attachment.name])}
          onClick={(e) => {
            e.stopPropagation();
            onRemove();
          }}
          className={`grid shrink-0 place-items-center rounded-full text-content/70 hover:bg-content/15 hover:text-content ${
            image
              ? "absolute -right-1 -top-1 size-5 bg-content/20 opacity-100 shadow-sm backdrop-blur-sm"
              : "size-4 text-content/40"
          }`}
        >
          <X className={image ? "size-3" : "size-3"} strokeWidth={2} />
        </button>
      ) : null}
    </div>
  );
}
