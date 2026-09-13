import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { LAYER } from "../lib/layers";
import {
  linkedWorkItemActivityPrompt,
  linkedWorkItemTerminalState,
  linkedWorkItemUpdateSummary,
  type LinkedWorkItemActivityEntry,
  type LinkedWorkItemUpdateCard,
} from "../lib/linkedWorkItemActivity";
import { formatRelativeTime } from "../lib/githubTasks";
import { playCue } from "../lib/sounds";
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
} from "./icons";

type Props = {
  card?: LinkedWorkItemUpdateCard;
  topOffset?: number;
  onAcknowledge: () => void;
  onDismiss: () => void;
  onOpenDiscussion: () => void;
  onAddToChat: (text: string) => void;
  onArchiveSession?: () => Promise<boolean>;
  onDeleteSession?: () => Promise<boolean>;
  onHeightChange?: (height: number) => void;
};

function entryKindLabel(entry: LinkedWorkItemActivityEntry): string {
  switch (entry.kind) {
    case "commit":
      return `Commit ${entry.id.slice(0, 7)}`;
    case "review":
      return "Review";
    case "review_comment":
      return "Review comment";
    default:
      return "Comment";
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
  card,
  topOffset = 12,
  onAcknowledge,
  onDismiss,
  onOpenDiscussion,
  onAddToChat,
  onArchiveSession,
  onDeleteSession,
  onHeightChange,
}: Props) {
  const panelRef = useRef<HTMLElement>(null);
  const soundedCards = useRef(new Set<string>());
  const [cleanupAction, setCleanupAction] = useState<
    "archive" | "delete" | undefined
  >();
  useLayoutEffect(() => {
    const panel = panelRef.current;
    if (!panel || !onHeightChange) return;
    const measure = () => onHeightChange(panel.offsetHeight);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(panel);
    return () => {
      observer.disconnect();
      onHeightChange(0);
    };
  }, [card, onHeightChange]);
  useEffect(() => {
    if (!card || card.status !== "ready") return;
    const key = `${card.kind}:${card.repo}:${card.number}:${card.updatedAt}`;
    if (soundedCards.current.has(key)) return;
    soundedCards.current.add(key);
    playCue("linkedActivity");
  }, [card]);
  if (!card || card.status === "loading") return null;

  const KindIcon = card.kind === "pr" ? GitPullRequest : CircleDot;
  const kindLabel = card.kind === "pr" ? "Pull request" : "Issue";
  const latest = card.entries[0];
  const discussion =
    latest?.kind === "comment" ||
    latest?.kind === "review" ||
    latest?.kind === "review_comment";
  const primaryLabel = discussion
    ? "Open discussion"
    : latest?.kind === "commit"
      ? "Open commit"
      : `Open ${card.kind === "pr" ? "pull request" : "issue"}`;
  const agentLabel = discussion
    ? "Address with agent"
    : latest?.kind === "commit"
      ? "Review with agent"
      : "Continue with agent";
  const terminalState = linkedWorkItemTerminalState(card);
  const terminalLabel =
    terminalState === "pr_merged"
      ? "Pull request merged"
      : terminalState === "pr_closed"
        ? "Pull request closed"
        : terminalState === "issue_closed"
          ? "Issue closed"
          : "";
  const TerminalIcon =
    terminalState === "pr_merged"
      ? GitMerge
      : terminalState === "pr_closed"
        ? GitPullRequestClosed
        : Check;

  const openPrimary = () => {
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

  return createPortal(
    <section
      ref={panelRef}
      aria-label={`New activity on ${kindLabel} ${card.number}`}
      aria-live="polite"
      style={{ zIndex: LAYER.popover - 1, top: topOffset }}
      className="linked-activity-notice fixed right-3 isolate w-[min(320px,calc(100vw-24px))] overflow-hidden rounded-xl border border-content/10 text-content shadow-xl"
    >
      <div
        aria-hidden="true"
        className="popover-backdrop pointer-events-none absolute inset-0 z-0 backdrop-blur-xl [backface-visibility:hidden] [transform:translateZ(0)]"
      />
      <div className="relative z-[1] flex items-center justify-between gap-0.5 border-b border-content/10 px-3 py-2">
        <div className="flex items-center gap-1.5">
          <span className="size-2 shrink-0 rounded-full bg-accent" />
          <KindIcon className="size-3.5 text-content/55" strokeWidth={1.75} />
          <span className="min-w-0 flex-1 truncate text-[12px] font-semibold">
            GitHub activity
          </span>
        </div>
        <div className="flex items-center gap-px">
          <button
            type="button"
            onClick={dismiss}
            className="shrink-0 whitespace-nowrap rounded-md px-1.5 h-6 text-[11px] text-content/50 hover:bg-content/10 hover:text-content"
          >
            Dismiss
          </button>
          <button
            type="button"
            title="Dismiss"
            aria-label={`Dismiss updates for ${kindLabel} ${card.number}`}
            onClick={dismiss}
            className="grid size-6 place-items-center rounded-md text-content/40 hover:bg-content/10 hover:text-content"
          >
            <X className="size-3" strokeWidth={2} />
          </button>
        </div>
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
        <div className="relative z-[1] max-h-52 overflow-y-auto border-t border-content/10 divide-y divide-content/10">
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
                    {entryKindLabel(entry)}
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
                  {entry.text || "No message"}
                </span>
              </span>
            </button>
          ))}
        </div>
      ) : null}

      {terminalState ? (
        <div className="relative z-[1] border-t border-content/10 px-3 py-2.5">
          <div className="flex items-center gap-2 text-[11px]">
            <TerminalIcon
              className={
                terminalState === "pr_merged"
                  ? "size-3.5 shrink-0 text-violet-400/90"
                  : "size-3.5 shrink-0 text-emerald-400/90"
              }
              strokeWidth={1.75}
            />
            <span className="font-medium text-content/75">{terminalLabel}</span>
            <span className="text-content/45">Clean up this session</span>
          </div>
          <div className="mt-2 flex items-center gap-1.5">
            <button
              type="button"
              title="Archive session"
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
                Archive session
              </span>
            </button>
            <button
              type="button"
              title="Delete session"
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
                Delete…
              </span>
            </button>
          </div>
        </div>
      ) : null}

      <div className="relative z-[1] flex min-w-0 items-center gap-1.5 border-t border-content/10 px-3 py-2.5 text-[11px]">
        <button
          type="button"
          title={primaryLabel}
          className="min-w-0 flex-1 truncate whitespace-nowrap rounded-md bg-content/10 px-2 py-1 font-medium hover:bg-content/15"
          onClick={openPrimary}
        >
          {primaryLabel}
        </button>
        <button
          type="button"
          title={agentLabel}
          className="min-w-0 flex-1 truncate whitespace-nowrap rounded-md px-2 py-1 text-content/65 hover:bg-content/10"
          onClick={() => {
            onAcknowledge();
            onAddToChat(linkedWorkItemActivityPrompt(card));
          }}
        >
          {agentLabel}
        </button>
      </div>
    </section>,
    document.body,
  );
}
