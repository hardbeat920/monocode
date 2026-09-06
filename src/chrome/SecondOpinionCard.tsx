import { ChevronRight } from "./icons";
import { HarnessIcon } from "./HarnessIcon";
import { t } from "../lib/i18n";
import { getHarnessTitle, type SecondOpinionMeta } from "../lib/session";

type Props = {
  card: SecondOpinionMeta;
};

export function SecondOpinionCard({ card }: Props) {
  const files =
    card.files != null && card.files > 0
      ? card.files === 1 ? t("{0} file", [String(card.files)]) : t("{0} files", [String(card.files)])
      : null;

  return (
    <div className="min-w-0 font-sans">
      <div className="text-[13px] font-medium leading-snug text-content">
        {card.kind === "handoff" ? t("Handoff") : t("Second opinion")}
      </div>
      <div className="mt-1 flex min-w-0 items-center gap-1.5 text-[11px] leading-4 text-content/50">
        <HarnessIcon harness={card.from} className="size-3 shrink-0" />
        <span className="truncate">{getHarnessTitle()[card.from]}</span>
        <ChevronRight
          className="size-3 shrink-0 text-content/35"
          strokeWidth={1.75}
        />
        <HarnessIcon harness={card.to} className="size-3 shrink-0" />
        <span className="truncate">{getHarnessTitle()[card.to]}</span>
      </div>
      {files ? (
        <div className="mt-1 text-[11px] leading-4 text-content/45">
          {files}
        </div>
      ) : null}
    </div>
  );
}
