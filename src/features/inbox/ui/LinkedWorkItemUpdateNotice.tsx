import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useState } from "react";
import {
  linkedWorkItemActivityPrompt,
  linkedWorkItemTerminalState,
  linkedWorkItemUpdateSummary,
  type LinkedWorkItemActivityEntry,
  type LinkedWorkItemUpdateCard,
} from "../model/linkedWorkItemActivity";
import { formatRelativeTime } from "../model/githubTasks";
import { announceLinkedActivity } from "../../settings/model/sounds";
import { GlassBackdrop } from "../../../app/shell/GlassBackdrop";
import {
  Archive,
  Check,
  CircleDot,
  GitBranch,
  GitMerge,
  GitPullRequest,
  GitPullRequestClosed,
  Loader,
  MessageSquare,
  Trash2,
  X,
} from "../../../shared/ui/icons";
import { useT } from "../../../shared/i18n/useT";
import type { TranslateFn } from "../../../shared/i18n/locale";

type Props = {
  sessionId: string;
  card?: LinkedWorkItemUpdateCard;
  onAcknowledge: () => void;
  onDismiss: () => void;
  onOpenDiscussion: () => void;
  onAddToChat: (text: string) => void;
  onArchiveSession?: () => Promise<boolean>;
  onDeleteSession?: () => Promise<boolean>;
};

function entryKindLabel(
  entry: LinkedWorkItemActivityEntry,
  t: TranslateFn,
): string {
  switch (entry.kind) {
    case "commit":
      return t("inbox.linked.commit", { id: entry.id.slice(0, 7) });
    case "review":
      return t("inbox.linked.review");
    case "review_comment":
      return t("inbox.linked.reviewComment");
    default:
      return t("inbox.linked.comment");
  }
}

function ActivityIcon({ entry }: { entry: LinkedWorkItemActivityEntry }) {
  if (entry.kind === "commit") {
    return <GitBranch className="size-3.5 shrink-0" strokeWidth={1.75} />;
  }
  if (entry.kind === "review") {
    return <Check className="size-3.5 shrink-0" strokeWidth={1.75} />;
  }
  return <MessageSquare className="size-3.5 shrink-0" strokeWidth={1.75} />;
}

export function LinkedWorkItemUpdateNotice({
  sessionId,
  card,
  onAcknowledge,
  onDismiss,
  onOpenDiscussion,
  onAddToChat,
  onArchiveSession,
  onDeleteSession,
}: Props) {
  const t = useT();
  const [cleanupAction, setCleanupAction] = useState<
    "archive" | "delete" | undefined
  >();
  useEffect(() => {
    announceLinkedActivity(sessionId, card);
  }, [sessionId, card]);
  if (!card || card.status === "loading") return null;

  const KindIcon = card.kind === "pr" ? GitPullRequest : CircleDot;
  const kindLabel =
    card.kind === "pr" ? t("inbox.kind.pullRequest") : t("inbox.kind.issue");
  const latest = card.entries[0];
  const discussion =
    latest?.kind === "comment" ||
    latest?.kind === "review" ||
    latest?.kind === "review_comment";
  const openLabel =
    latest?.kind === "commit"
      ? t("inbox.linked.openCommit")
      : card.kind === "pr"
        ? t("inbox.linked.openPr")
        : t("inbox.linked.openIssue");
  const agentLabel = discussion
    ? t("inbox.linked.addressWithAgent")
    : latest?.kind === "commit"
      ? t("inbox.linked.reviewWithAgent")
      : t("inbox.linked.continueWithAgent");
  const terminalState = linkedWorkItemTerminalState(card);
  const terminalLabel =
    terminalState === "pr_merged"
      ? t("inbox.linked.prMerged")
      : terminalState === "pr_closed"
        ? t("inbox.linked.prClosed")
        : terminalState === "issue_closed"
          ? t("inbox.linked.issueClosed")
          : "";
  const TerminalIcon =
    terminalState === "pr_merged"
      ? GitMerge
      : terminalState === "pr_closed"
        ? GitPullRequestClosed
        : Check;

  const openActivity = () => {
    onAcknowledge();
    if (discussion) {
      onOpenDiscussion();
      return;
    }
    void openUrl(latest?.url || card.url);
  };
  const dismiss = () => {
    onAcknowledge();
    onDismiss();
  };
  const runCleanup = async (
    action: "archive" | "delete",
    handler: (() => Promise<boolean>) | undefined,
  ) => {
    if (!handler || cleanupAction) return;
    setCleanupAction(action);
    try {
      if (await handler()) onAcknowledge();
    } finally {
      setCleanupAction(undefined);
    }
  };

  return (
    <section
      aria-label={t("inbox.linked.newActivity", {
        kind: kindLabel,
        number: card.number,
      })}
      aria-live="polite"
      className="pointer-events-auto absolute top-3 right-3 z-40 isolate w-[min(320px,calc(100%_-_24px))] overflow-hidden rounded-xl border border-content/10 text-content shadow-xl"
    >
      <GlassBackdrop />
      <div className="linked-activity-notice relative z-[1]">
        <div className="relative z-[1] flex items-center justify-between gap-0.5 border-b border-stroke px-3 py-2">
          <div className="flex items-center gap-1.5">
            <span className="size-2 shrink-0 rounded-full bg-accent" />
            <KindIcon className="size-3.5 text-content/55" strokeWidth={1.75} />
            <span className="min-w-0 flex-1 truncate text-[12px] font-semibold">
              {t("inbox.linked.githubActivity")}
            </span>
          </div>
          <button
            type="button"
            title={t("inbox.linked.dismiss")}
            aria-label={t("inbox.linked.dismissUpdates", {
              kind: kindLabel,
              number: card.number,
            })}
            onClick={dismiss}
            className="grid size-6 shrink-0 place-items-center rounded-md text-content/40 hover:bg-content/10 hover:text-content"
          >
            <X className="size-3" strokeWidth={2} />
          </button>
        </div>

        <div className="relative z-[1] px-3 py-2.5">
          <button
            type="button"
            onClick={() => {
              onAcknowledge();
              void openUrl(card.url);
            }}
            className="block w-full text-left"
          >
            <span className="block text-[11px] text-content/50">
              {kindLabel} #{card.number} · {card.repo}
            </span>
            <span className="mt-0.5 block truncate text-[13px] font-medium hover:underline">
              {card.title}
            </span>
          </button>
          <div className="mt-1.5 flex items-center gap-1.5 text-[11px] text-content/55">
            <span>{linkedWorkItemUpdateSummary(card)}</span>
          </div>
        </div>

        {card.entries.length > 0 ? (
          <div className="relative z-[1] max-h-52 overflow-y-auto border-t border-stroke divide-y divide-stroke">
            {card.entries.slice(0, 3).map((entry) => (
              <button
                key={`${entry.kind}:${entry.id}`}
                type="button"
                disabled={!entry.url}
                onClick={() => {
                  if (entry.url) {
                    onAcknowledge();
                    void openUrl(entry.url);
                  }
                }}
                className="flex w-full items-start gap-2 px-3 py-2 text-left hover:bg-content/5 disabled:cursor-default disabled:hover:bg-transparent"
              >
                <span className="mt-0.5 text-content/45">
                  <ActivityIcon entry={entry} />
                </span>
                <span className="min-w-0 flex-1">
                  <span className="flex min-w-0 items-center gap-1.5 text-[11px]">
                    <span className="font-medium text-content/70">
                      {entryKindLabel(entry, t)}
                    </span>
                    {entry.author ? (
                      <span className="min-w-0 truncate text-content/45">
                        @{entry.author}
                      </span>
                    ) : null}
                    <span className="ml-auto shrink-0 text-content/35">
                      {formatRelativeTime(entry.createdAt)}
                    </span>
                  </span>
                  <span className="mt-0.5 line-clamp-2 block text-[12px] leading-relaxed text-content/65">
                    {entry.text || t("inbox.linked.noMessage")}
                  </span>
                </span>
              </button>
            ))}
          </div>
        ) : null}

        {terminalState ? (
          <div className="relative z-[1] border-t border-stroke px-3 py-2.5">
            <div className="flex items-center gap-2 text-[11px]">
              <TerminalIcon
                className={
                  terminalState === "pr_merged"
                    ? "size-3.5 shrink-0 text-violet-400/90"
                    : "size-3.5 shrink-0 text-emerald-400/90"
                }
                strokeWidth={1.75}
              />
              <span className="font-medium text-content/75">
                {terminalLabel}
              </span>
              <span className="text-content/45">
                {t("inbox.linked.cleanUp")}
              </span>
            </div>
            <div className="mt-2 flex items-center gap-1.5">
              <button
                type="button"
                title={t("inbox.linked.archiveSession")}
                disabled={Boolean(cleanupAction) || !onArchiveSession}
                onClick={() => void runCleanup("archive", onArchiveSession)}
                className="inline-flex min-w-0 items-center gap-1.5 overflow-hidden rounded-md bg-content/10 px-2 py-1 text-[11px] font-medium hover:bg-content/15 disabled:opacity-40"
              >
                {cleanupAction === "archive" ? (
                  <Loader className="size-3 shrink-0 animate-spin" />
                ) : (
                  <Archive className="size-3 shrink-0" strokeWidth={1.75} />
                )}
                <span className="min-w-0 truncate whitespace-nowrap">
                  {t("inbox.linked.archiveSession")}
                </span>
              </button>
              <button
                type="button"
                title={t("inbox.linked.deleteSession")}
                disabled={Boolean(cleanupAction) || !onDeleteSession}
                onClick={() => void runCleanup("delete", onDeleteSession)}
                className="inline-flex min-w-0 items-center gap-1.5 overflow-hidden rounded-md px-2 py-1 text-[11px] text-red-300/90 hover:bg-red-500/15 disabled:opacity-40"
              >
                {cleanupAction === "delete" ? (
                  <Loader className="size-3 shrink-0 animate-spin" />
                ) : (
                  <Trash2 className="size-3 shrink-0" strokeWidth={1.75} />
                )}
                <span className="min-w-0 truncate whitespace-nowrap">
                  {t("inbox.linked.delete")}
                </span>
              </button>
            </div>
          </div>
        ) : null}

        <div className="relative z-[1] flex min-w-0 items-center gap-1.5 border-t border-stroke px-3 py-2.5 text-[11px]">
          <button
            type="button"
            title={agentLabel}
            className="min-w-0 flex-1 truncate whitespace-nowrap rounded-md bg-content px-2 py-1 font-medium text-background-base hover:bg-content/90"
            onClick={() => {
              onAcknowledge();
              onAddToChat(linkedWorkItemActivityPrompt(card));
            }}
          >
            {agentLabel}
          </button>
          <button
            type="button"
            title={openLabel}
            className="min-w-0 flex-1 truncate whitespace-nowrap rounded-md bg-content/10 px-2 py-1 font-medium hover:bg-content/15"
            onClick={openActivity}
          >
            {openLabel}
          </button>
        </div>
      </div>
    </section>
  );
}
