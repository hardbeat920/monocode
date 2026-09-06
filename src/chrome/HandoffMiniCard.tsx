import { ChevronRight, Replace, X } from "./icons";
import { HarnessIcon } from "./HarnessIcon";
import { t } from "../lib/i18n";
import { getHarnessTitle, type HarnessId } from "../lib/session";

type Card = {
  from: HarnessId;
  to: HarnessId;
  request?: string;
  files?: number;
};

type Props = {
  card: Card;
  onDismiss?: () => void;
};

export function HandoffMiniCard({ card, onDismiss }: Props) {
  const files =
    card.files != null && card.files > 0
      ? card.files === 1 ? t("1 file") : t("{0} files", [String(card.files)])
      : null;

  return (
    <div className="px-3 pt-2">
      <div
        className={`relative rounded-md border border-content/10 bg-content/6 px-2.5 py-2 ${
          onDismiss ? "pr-8" : ""
        }`}
      >
        <div className="flex w-full flex-col text-left">
          <span className="flex min-w-0 items-center gap-1.5">
            <Replace
              className="size-3.5 shrink-0 text-content/45"
              strokeWidth={1.75}
            />
            <span className="min-w-0 truncate text-[11px] text-content/50">
              {t("Handoff")}
            </span>
          </span>
          <span className="mt-1 flex min-w-0 items-center gap-1.5 text-[13px] font-semibold leading-snug text-content">
            <HarnessIcon harness={card.from} className="size-3.5 shrink-0" />
            <span className="min-w-0 truncate">{getHarnessTitle()[card.from]}</span>
            <ChevronRight
              className="size-3 shrink-0 text-content/35"
              strokeWidth={1.75}
            />
            <HarnessIcon harness={card.to} className="size-3.5 shrink-0" />
            <span className="min-w-0 truncate">{getHarnessTitle()[card.to]}</span>
          </span>
          {card.request ? (
            <span className="mt-1 line-clamp-1 text-[11px] text-content/45">
              {card.request}
            </span>
          ) : null}
          {files ? (
            <span className="mt-1 text-[11px] leading-4 text-content/45">
              {files}
            </span>
          ) : null}
        </div>
        {onDismiss ? (
          <button
            type="button"
            title={t("Remove")}
            aria-label={t("Remove handoff")}
            onClick={onDismiss}
            className="absolute right-1.5 top-1.5 grid size-5 place-items-center rounded text-content/40 hover:bg-content/10 hover:text-content"
          >
            <X className="size-3" strokeWidth={2} />
          </button>
        ) : null}
      </div>
    </div>
  );
}
