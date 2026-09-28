import { useCallback, useEffect, useRef, useState } from "react";
import {
  Check,
  Clock,
  Copy,
  GitBranch,
  Globe,
  Pin,
} from "../../../shared/ui/icons";
import {
  Popover,
  type PopoverAnchor,
  type PopoverDismissReason,
} from "../../../shared/ui/Popover";
import { copyText } from "../../../platform/tauri/clipboard";
import type { GitHistoryCommit } from "../../../platform/tauri/fs";
import { loadCommitMessage, type CommitMessage } from "../model/commitMessage";
import { useCommitStats } from "../hooks/useCommitStats";
import { useCommitMessage } from "../hooks/useCommitMessage";
import { formatCommitTimestamp } from "../model/commitDate";
import type { GraphRef } from "../model/gitGraph";

type Props = {
  cwd: string;
  commit: GitHistoryCommit;
  refs: GraphRef[];
  anchor: PopoverAnchor;
  id: string;
  onDismiss: (reason: PopoverDismissReason) => void;
  onFocusLeave: (next: EventTarget | null) => void;
  onReturnFocus: () => void;
  onTabForward: () => boolean;
  onPointerEnter: () => void;
  onPointerLeave: () => void;
};

/**
 * How long to wait for the commit message before showing the card anyway. Git
 * usually answers in well under this; the budget only stops a slow repository
 * from leaving the card invisible, and costs nothing when the message is
 * already cached. Exported so a test can hold the reveal to the same number
 * rather than a copy of it.
 */
export const REVEAL_BUDGET_MS = 150;

/** No project folder means no Git to ask, so there is nothing to wait for. */
function hasRepo(cwd: string): boolean {
  return Boolean(cwd) && cwd !== "~";
}

/**
 * Whether to paint the card, given whether the content it is waiting on has
 * settled.
 *
 * The card animates in over 170ms, and its body arrives from a Git call a
 * moment later. Painting straight away means the animation plays on a card
 * with no body, which then grows by up to 240px under the reader — a jump
 * rather than a reveal. Holding the paint until the body is in hand trades a
 * few tens of milliseconds of hover latency for one clean animation of the
 * finished card.
 *
 * The row reports `aria-expanded` for the whole window, so for up to
 * `REVEAL_BUDGET_MS` it points at a card that is not on screen yet.
 */
function useRevealWhenSettled(settled: boolean): boolean {
  const [expired, setExpired] = useState(false);
  useEffect(() => {
    if (settled) return;
    const timer = setTimeout(() => setExpired(true), REVEAL_BUDGET_MS);
    return () => clearTimeout(timer);
  }, [settled]);
  return settled || expired;
}

/**
 * Hover card for a history row, in the shape VS Code's GitLens card uses:
 * author and date on one line, the full message, a changed-file summary,
 * ref chips, then the SHA with copy actions. No avatar and no remote link.
 */
export function CommitHoverCard({
  cwd,
  commit,
  refs,
  anchor,
  id,
  onDismiss,
  onFocusLeave,
  onReturnFocus,
  onTabForward,
  onPointerEnter,
  onPointerLeave,
}: Props) {
  const timestamp = formatCommitTimestamp(commit.timestamp);
  // The message is the card's height. Fetched here rather than in the body so
  // the reveal can wait for it, which is what keeps the card from painting
  // once without its body and then jumping as the text lands.
  const message = useCommitMessage(cwd, commit.sha);
  const revealed = useRevealWhenSettled(message !== undefined || !hasRepo(cwd));

  if (!revealed) return null;

  return (
    <Popover
      anchor={anchor}
      side="right"
      align="start"
      gap={6}
      constrainHeight={false}
      onDismiss={onDismiss}
      id={id}
      role="dialog"
      aria-label={`Commit ${commit.shortSha} details`}
      // A fixed width, capped by the frame, the way VS Code sizes its issue
      // hover (`width: 621px; max-width: 100%`). A card that shrink-wraps
      // changes width on every row in the list — no refs versus three long
      // branch names, "fix" versus a long body — so it flickers as the pointer
      // moves down. `max-w-full` is what keeps the fixed width from being
      // clipped when the window is narrower than the card; the Popover frame
      // carries `overflow-hidden`, and `placePopover` clamps that frame to the
      // viewport, so the cap belongs here in CSS rather than in a second
      // viewport calculation.
      //
      // VS Code's core editor hover uses a single `max-width` instead, but
      // picks 500px rather than shrink-wrapping. Raising this to 500px would
      // match it exactly; 28rem leaves a card that opens off a narrow sidebar a
      // little less screen to cover.
      className="flex w-[28rem] max-w-full flex-col gap-1.5 p-3 font-sans text-left text-content"
      onBlur={(event) => {
        const next = event.relatedTarget;
        if (!(next instanceof Node) || !event.currentTarget.contains(next))
          onFocusLeave(next);
      }}
      onKeyDown={(event) => {
        if (event.key !== "Tab") return;
        const buttons = event.currentTarget.querySelectorAll("button");
        if (event.shiftKey && event.target === buttons[0]) {
          event.preventDefault();
          onReturnFocus();
        } else if (
          !event.shiftKey &&
          event.target === buttons[buttons.length - 1]
        ) {
          if (onTabForward()) event.preventDefault();
        }
      }}
      onMouseEnter={onPointerEnter}
      onMouseLeave={onPointerLeave}
    >
      <CommitHeader author={commit.author} timestamp={timestamp} />

      {commit.subject ? (
        <p className="break-words text-[12px] leading-[1.45] text-content/85">
          {commit.subject}
        </p>
      ) : null}

      <CommitDescription message={message} />

      <CommitStatsLine cwd={cwd} sha={commit.sha} />

      {refs.length > 0 ? (
        <div className="flex flex-wrap gap-1">
          {refs.map((ref) => (
            <RefChip key={`${ref.kind}:${ref.name}`} refInfo={ref} />
          ))}
        </div>
      ) : null}
      <div className="flex min-w-0 items-center gap-1 border-t border-content/[0.07] pt-1.5">
        <code className="shrink-0 font-mono text-[11px] leading-4 text-content/70">
          {commit.shortSha}
        </code>
        <CopyButton value={commit.sha} label="Copy commit SHA" />
        <span className="shrink-0 px-1 text-content/25" aria-hidden="true">
          |
        </span>
        <CopyMessageButton
          cwd={cwd}
          sha={commit.sha}
          fallback={commit.subject || commit.shortSha}
        />
      </div>
    </Popover>
  );
}

/** Author and date on one wrapped line; the widest thing the card sizes to. */
function CommitHeader({
  author,
  timestamp,
}: {
  author: string;
  timestamp: string;
}) {
  return (
    <div className="flex min-w-0 max-w-full flex-wrap items-center gap-1.5 text-[11px] leading-4">
      {author ? (
        <>
          <span
            className="min-w-0 break-words font-medium text-content/80"
            title={author}
          >
            {author}
          </span>
          <span className="shrink-0 text-content/30">,</span>
        </>
      ) : null}
      <Clock className="size-3.5 shrink-0 text-content/45" aria-hidden="true" />
      <span className="min-w-0 break-words text-content/55">
        {timestamp || "Date unavailable"}
      </span>
    </div>
  );
}

/**
 * The body under the subject. The card grows to show all of it; past
 * `max-h-60` the block scrolls inside the card rather than pushing it off
 * screen. Takes the message from the card, which already has it in hand.
 */
function CommitDescription({
  message,
}: {
  // Still undefined if the reveal budget expired before Git answered.
  message: CommitMessage | null | undefined;
}) {
  const description = message?.description;
  if (!description) return null;
  return (
    <div className="min-w-0 w-full max-h-60 overflow-y-auto overscroll-contain pr-1">
      <p className="whitespace-pre-wrap break-words text-[11.5px] leading-[1.55] text-content/60">
        {description}
      </p>
    </div>
  );
}

function CommitStatsLine({ cwd, sha }: { cwd: string; sha: string }) {
  const stats = useCommitStats(cwd, sha);

  if (stats === undefined) {
    return (
      // `h-4` matches the `leading-4` of the real line below, so the swap from
      // placeholder to text does not nudge the card's height.
      <span
        aria-hidden="true"
        className="block h-4 w-40 rounded bg-content/10 motion-safe:animate-pulse"
      />
    );
  }
  if (stats === null) {
    return (
      <p className="text-[11px] leading-4 text-content/40">
        Changed files unavailable
      </p>
    );
  }

  const { filesChanged, additions, deletions } = stats;
  return (
    <p className="flex flex-wrap items-baseline gap-x-1 text-[11px] leading-4">
      <span className="text-content/55">
        {filesChanged === 1
          ? "1 file changed"
          : `${filesChanged} files changed`}
      </span>
      {additions > 0 ? (
        <>
          <span className="text-content/25">,</span>
          <span className="font-medium tabular-nums text-emerald-400">
            {additions} insertion{additions === 1 ? "" : "s"}(+)
          </span>
        </>
      ) : null}
      {deletions > 0 ? (
        <>
          <span className="text-content/25">,</span>
          <span className="font-medium tabular-nums text-red-400">
            {deletions} deletion{deletions === 1 ? "" : "s"}(-)
          </span>
        </>
      ) : null}
    </p>
  );
}

function RefChip({ refInfo }: { refInfo: GraphRef }) {
  const Icon =
    refInfo.kind === "local"
      ? GitBranch
      : refInfo.kind === "remote"
        ? Globe
        : refInfo.kind === "tag"
          ? Pin
          : null;
  return (
    <span
      title={refInfo.name}
      className={`flex max-w-full min-w-0 items-center gap-1 rounded-full px-1.5 py-px text-[10px] leading-4 ${
        refInfo.color ? "" : "bg-content/10 text-content/60"
      }`}
      style={
        refInfo.color
          ? {
              backgroundColor: refInfo.color,
              color: "var(--color-background-base)",
            }
          : undefined
      }
    >
      {Icon ? (
        <Icon className="size-3 shrink-0" strokeWidth={2} aria-hidden="true" />
      ) : null}
      <span className="min-w-0 truncate">{refInfo.name}</span>
    </span>
  );
}

function CopyButton({ value, label }: { value: string; label: string }) {
  const { copied, copy } = useCopyFeedback();
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      onClick={() => copy(value)}
      className="grid size-5 shrink-0 place-items-center rounded text-content/45 hover:bg-content/10 hover:text-content focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
    >
      {copied ? (
        <Check className="size-3.5 text-emerald-400" aria-hidden="true" />
      ) : (
        <Copy className="size-3.5" aria-hidden="true" />
      )}
    </button>
  );
}

function CopyMessageButton({
  cwd,
  sha,
  fallback,
}: {
  cwd: string;
  sha: string;
  fallback: string;
}) {
  const { copied, copy } = useCopyFeedback();
  return (
    <button
      type="button"
      aria-label="Copy commit message"
      onClick={() => {
        void loadCommitMessage(cwd, sha).then((message) =>
          copy(message?.raw ?? fallback),
        );
      }}
      className="flex min-w-0 shrink items-center gap-1 rounded px-1 py-0.5 text-left text-[11px] leading-4 text-content/50 hover:bg-content/10 hover:text-content focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-accent/60"
    >
      {copied ? (
        <Check
          className="size-3.5 shrink-0 text-emerald-400"
          aria-hidden="true"
        />
      ) : (
        <Copy className="size-3.5 shrink-0" aria-hidden="true" />
      )}
      <span className="truncate">{copied ? "Copied" : "Copy message"}</span>
    </button>
  );
}

function useCopyFeedback(): { copied: boolean; copy: (value: string) => void } {
  const [copied, setCopied] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    return () => {
      if (timer.current != null) clearTimeout(timer.current);
    };
  }, []);

  const copy = useCallback((value: string) => {
    void copyText(value)
      .then(() => {
        setCopied(true);
        if (timer.current != null) clearTimeout(timer.current);
        timer.current = setTimeout(() => {
          timer.current = null;
          setCopied(false);
        }, 1200);
      })
      .catch(() => undefined);
  }, []);

  return { copied, copy };
}
