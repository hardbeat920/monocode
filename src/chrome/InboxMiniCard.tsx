import { CircleDot, GitPullRequest, X } from "./icons";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { GithubLabel, InboxComposerCard } from "../lib/githubTasks";
import { t } from "../lib/i18n";
import { InboxProviderMark } from "./InboxProviderMark";

type Props = {
  card: InboxComposerCard;
  onDismiss?: () => void;
};

export function InboxMiniCard({ card, onDismiss }: Props) {
  const KindIcon = card.kind === "pr" ? GitPullRequest : CircleDot;
  const kindLabel = card.kind === "pr" ? t("Pull request") : t("Issue");
  const providerLabel = card.provider === "linear" ? "Linear" : "GitHub";

  return (
    <div className="px-3 pt-2">
      <div className="relative rounded-md border border-content/10 bg-content/6 px-2.5 py-2 pr-8">
        <button
          type="button"
          title={t("Open in {0}", [providerLabel])}
          aria-label={t("Open {0} {1} in {2}", [kindLabel, card.identifier, providerLabel])}
          disabled={!card.url}
          onClick={() => {
            if (card.url) void openUrl(card.url);
          }}
          className="flex w-full flex-col text-left disabled:cursor-default"
        >
          <span className="flex min-w-0 items-center gap-1.5">
            <InboxProviderMark
              provider={card.provider}
              className="size-3.5 shrink-0"
            />
            <KindIcon
              className="size-3 shrink-0 text-content/45"
              strokeWidth={1.75}
            />
            <span className="min-w-0 truncate text-[11px] text-content/50">
              {kindLabel} · {card.identifier}
            </span>
          </span>
          <span className="mt-1 line-clamp-1 text-[13px] font-semibold leading-snug text-content">
            {card.title}
          </span>
          <span className="mt-1 flex min-w-0 items-center gap-2">
            {card.source ? (
              <span className="min-w-0 flex-1 truncate text-[11px] text-content/45">
                {card.source}
              </span>
            ) : (
              <span className="min-w-0 flex-1" />
            )}
            {card.labels.length > 0 ? (
              <span className="flex min-w-0 shrink-0 items-center gap-1">
                {card.labels.map((label) => (
                  <InboxMiniLabel key={label.name} label={label} />
                ))}
              </span>
            ) : null}
          </span>
        </button>
        {onDismiss ? (
          <button
            type="button"
            title={t("Remove")}
            aria-label={t("Remove {0} {1}", [kindLabel, card.identifier])}
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

function InboxMiniLabel({ label }: { label: GithubLabel }) {
  const color = labelColor(label.color);
  return (
    <span className="inline-flex min-w-0 max-w-20 items-center gap-1 rounded bg-content/8 px-1.5 py-px text-[10px] text-content/50">
      {color ? (
        <span
          aria-hidden
          className="size-1.5 shrink-0 rounded-full"
          style={{ backgroundColor: color }}
        />
      ) : null}
      <span className="min-w-0 truncate">{label.name}</span>
    </span>
  );
}

function labelColor(value: string): string | null {
  const hex = value.trim().replace(/^#/, "");
  if (!/^[0-9a-fA-F]{6}$/.test(hex)) return null;
  return `#${hex}`;
}
