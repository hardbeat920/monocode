import {
  Brain,
  Check,
  ChevronRight,
  Clock,
  CircleDashed,
  Copy,
  Hammer,
  Lightbulb,
  Pencil,
  Search,
  Terminal,
  X,
} from "lucide-react";
import {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { AttachmentChip } from "../chrome/AttachmentChip";
import { FilePreview } from "../chrome/FilePreview";
import { FileTypeIcon } from "../chrome/FileTypeIcon";
import { PlanPreview } from "../chrome/PlanPreview";
import { SecondOpinionButton } from "../chrome/SecondOpinionButton";
import { SecondOpinionCard } from "../chrome/SecondOpinionCard";
import { TerminalSpinner } from "../chrome/TerminalSpinner";
import type { ApprovalDecision } from "../lib/harness";
import {
  isEditTool,
  isReadTool,
  isSearchTool,
  stubFilePreview,
} from "../lib/harness/preview";
import { copyText } from "../lib/clipboard";
import { displayPath, resolveWorkspacePath } from "../lib/paths";
import { harnessForTurn } from "../lib/secondOpinion";
import { Shimmer } from "./Shimmer";
import {
  hasPendingApproval,
  HARNESS_TITLE,
  type Block,
  type HarnessId,
  type ToolPreview,
} from "../lib/session";
import { HarnessIcon } from "../chrome/HarnessIcon";
import { useLockOverscroll } from "../hooks/useLockOverscroll";
import { useTranscriptLayout } from "../hooks/useTranscriptLayout";
import { useTranscriptZen } from "../hooks/useTranscriptZen";
import { useActivityTicker } from "../hooks/useActivityTicker";
import { useTranscriptSelection } from "../hooks/useTranscriptSelection";
import type { TranscriptLayout } from "../lib/appearance";
import { AgentMarkdown } from "./AgentMarkdown";
import { TranscriptSelectionMenu } from "./TranscriptSelectionMenu";
import {
  activityGroupView,
  activityPreviousCount,
  activityPreviousLabel,
  activitySummary,
  editVerb,
  groupTurnItems,
  groupTurns,
  isIncompleteTool,
  isThinkingBlock,
  lastActivityIndex,
  isProseBlock,
  needsApproval,
  proseSummary,
  splitActivityRows,
  toolCallLabel,
  toolCallState,
  turnCopyText,
  type ToolCallState,
} from "./transcriptActivity";

const NEAR_BOTTOM_PX = 16;
/** Matches the .zen-ticker-out animation, after which the old row is dropped. */
const TICKER_EXIT_MS = 300;
const INITIAL_TURNS = 20;
const TURN_PAGE_SIZE = 20;

type Props = {
  blocks: Block[];
  busy?: boolean;
  cwd?: string;
  harness?: HarnessId;
  onApproval?: (requestId: number, decision: ApprovalDecision) => void;
  onAddToChat?: (text: string) => void;
  onOpenFile?: (path: string) => void;
  onOpenDiff?: (path: string) => void;
  onOpenPlan?: (blockId: string) => void;
  onSecondOpinion?: (harness: HarnessId, turn: Block[], model: string) => void;
  onJumpToBottomChange?: (show: boolean) => void;
  onJumpToBottomReady?: (jump: () => void) => void;
};

export function AgentTranscript({
  blocks,
  busy,
  cwd,
  harness,
  onApproval,
  onAddToChat,
  onOpenFile,
  onOpenDiff,
  onOpenPlan,
  onSecondOpinion,
  onJumpToBottomChange,
  onJumpToBottomReady,
}: Props) {
  const lockOverscroll = useLockOverscroll<HTMLDivElement>();
  const scroller = useRef<HTMLDivElement>(null);
  const stickToBottom = useRef(true);
  const showJumpRef = useRef(false);
  const distanceFromBottom = useRef(0);
  const prependHeight = useRef<number | null>(null);
  const [scrollerEl, setScrollerEl] = useState<HTMLDivElement | null>(null);
  const [visibleTurnCount, setVisibleTurnCount] = useState(INITIAL_TURNS);
  const { selection, dismissSelection } = useTranscriptSelection(
    scrollerEl,
    onAddToChat !== undefined,
  );
  const transcriptLayout = useTranscriptLayout();
  const zen = useTranscriptZen();
  const lastUserId = lastUserBlockId(blocks);
  const liveStartedAt = turnUserBlock(blocks)?.startedAt;
  const waitingForApproval = hasPendingApproval(blocks);
  const preparingHandoff = blocks.some(
    (block) =>
      block.role === "handoff" && block.handoff?.status === "preparing",
  );

  const setShowJump = useCallback(
    (show: boolean) => {
      if (showJumpRef.current === show) return;
      showJumpRef.current = show;
      onJumpToBottomChange?.(show);
    },
    [onJumpToBottomChange],
  );

  const syncPinned = useCallback(
    (el: HTMLElement) => {
      const near = isNearBottom(el);
      stickToBottom.current = near;
      distanceFromBottom.current =
        el.scrollHeight - el.scrollTop - el.clientHeight;
      setShowJump(!near);
    },
    [setShowJump],
  );

  const jumpToBottom = useCallback(() => {
    stickToBottom.current = true;
    distanceFromBottom.current = 0;
    setShowJump(false);
    pinToBottom(scroller.current);
  }, [setShowJump]);

  const setScroller = useCallback(
    (el: HTMLDivElement | null) => {
      scroller.current = el;
      setScrollerEl(el);
      lockOverscroll(el);
    },
    [lockOverscroll],
  );

  useEffect(() => {
    onJumpToBottomReady?.(jumpToBottom);
  }, [jumpToBottom, onJumpToBottomReady]);

  useEffect(() => {
    if (!scrollerEl) return;
    syncPinned(scrollerEl);
    const onScroll = () => syncPinned(scrollerEl);
    const onWheel = (e: WheelEvent) => {
      if (e.deltaY < 0) {
        stickToBottom.current = false;
        setShowJump(true);
      }
    };
    scrollerEl.addEventListener("scroll", onScroll, { passive: true });
    scrollerEl.addEventListener("wheel", onWheel, { passive: true });
    return () => {
      scrollerEl.removeEventListener("scroll", onScroll);
      scrollerEl.removeEventListener("wheel", onWheel);
    };
  }, [scrollerEl, setShowJump, syncPinned]);

  useLayoutEffect(() => {
    stickToBottom.current = true;
    setShowJump(false);
    pinToBottom(scroller.current);
  }, [lastUserId, setShowJump]);

  useLayoutEffect(() => {
    if (!stickToBottom.current) return;
    pinToBottom(scroller.current);
  }, [blocks, busy]);

  useEffect(() => {
    const el = scrollerEl;
    const inner = el?.firstElementChild;
    if (!el || !inner) return;
    const observer = new ResizeObserver(() => {
      const distance = el.scrollHeight - el.scrollTop - el.clientHeight;
      if (stickToBottom.current) {
        pinToBottom(el);
        distanceFromBottom.current = 0;
        return;
      }
      distanceFromBottom.current = distance;
      setShowJump(!isNearBottom(el));
    });
    observer.observe(inner);
    return () => observer.disconnect();
  }, [scrollerEl, setShowJump]);

  const turns = groupTurns(blocks);
  const firstVisibleTurn = Math.max(0, turns.length - visibleTurnCount);
  const visibleTurns = turns.slice(firstVisibleTurn);

  useLayoutEffect(() => {
    const previousHeight = prependHeight.current;
    const el = scroller.current;
    if (previousHeight == null || !el) return;
    prependHeight.current = null;
    el.scrollTop += el.scrollHeight - previousHeight;
    distanceFromBottom.current =
      el.scrollHeight - el.scrollTop - el.clientHeight;
  }, [visibleTurnCount]);

  const loadEarlier = () => {
    const el = scroller.current;
    if (el) prependHeight.current = el.scrollHeight;
    stickToBottom.current = false;
    setVisibleTurnCount((count) =>
      Math.min(turns.length, count + TURN_PAGE_SIZE),
    );
  };

  return (
    <div
      ref={setScroller}
      className="agent-transcript h-full overflow-y-auto overscroll-none [overflow-anchor:none] font-mono text-[13px] leading-5"
    >
      <div className="mx-auto flex w-full min-w-0 max-w-4xl flex-col gap-1 pb-1">
        {firstVisibleTurn > 0 ? (
          <div className="flex justify-center px-4 py-3">
            <button
              type="button"
              className="rounded-md bg-content/8 px-2.5 py-1.5 font-sans text-[12px] text-content/60 hover:bg-content/12 hover:text-content"
              onClick={loadEarlier}
            >
              Load earlier messages
            </button>
          </div>
        ) : null}
        {visibleTurns.map((turn, turnIndex) => {
          const isLastTurn = firstVisibleTurn + turnIndex === turns.length - 1;
          const durationMs = turnUserBlock(turn)?.durationMs;
          const settled = !(busy && isLastTurn);
          const items = groupTurnItems(turn, zen);
          // Zen hangs the turn's "Worked for" line on the fold that replaced
          // the ticker, so the footer under the answer keeps the clock only.
          const foldedAt = zen ? lastActivityIndex(items) : -1;
          const startedAt = turnUserBlock(turn)?.startedAt;
          // The agent starting its answer is the end of the work: fold the
          // stack then, not when the turn finally settles, so the collapse
          // never lands under the text you have already started reading.
          const answering =
            foldedAt >= 0 &&
            items
              .slice(foldedAt + 1)
              .some(
                (item) => item.type === "block" && isProseBlock(item.block),
              );
          return (
            <div
              key={turn[0].id}
              className={`transcript-turn flex min-w-0 flex-col gap-1${
                isLastTurn ? " transcript-turn-live" : ""
              }`}
            >
              {items.map((item, itemIndex) =>
                item.type === "activity" ? (
                  <ActivityGroup
                    key={item.blocks[0].id}
                    blocks={item.blocks}
                    cwd={cwd}
                    collapsed={zen && (settled || answering)}
                    zen={zen}
                    durationMs={itemIndex === foldedAt ? durationMs : undefined}
                    startedAt={
                      itemIndex === foldedAt && !settled ? startedAt : undefined
                    }
                    onApproval={onApproval}
                    onOpenFile={onOpenFile}
                  />
                ) : (
                  <TranscriptBlock
                    key={item.block.id}
                    block={item.block}
                    layout={transcriptLayout}
                    stickyIndex={firstVisibleTurn + turnIndex + 1}
                    compactTop={
                      foldedAt >= 0 &&
                      itemIndex === foldedAt + 1 &&
                      isProseBlock(item.block)
                    }
                    onApproval={onApproval}
                    onOpenFile={onOpenFile}
                    onOpenDiff={onOpenDiff}
                    onOpenPlan={onOpenPlan}
                    cwd={cwd}
                  />
                ),
              )}
              {durationMs != null && settled ? (
                <TurnDuration
                  elapsedMs={durationMs}
                  done
                  showElapsed={foldedAt < 0}
                  completedAt={
                    startedAt != null ? startedAt + durationMs : undefined
                  }
                  copyText={turnCopyText(turn)}
                  fromHarness={
                    harness ? harnessForTurn(blocks, turn, harness) : undefined
                  }
                  onSecondOpinion={
                    onSecondOpinion
                      ? (target, model) => onSecondOpinion(target, turn, model)
                      : undefined
                  }
                />
              ) : null}
              {busy && !preparingHandoff && isLastTurn && !answering ? (
                <LiveWorking
                  startedAt={liveStartedAt}
                  paused={waitingForApproval}
                />
              ) : null}
            </div>
          );
        })}
      </div>
      {onAddToChat ? (
        <TranscriptSelectionMenu
          selection={selection}
          onAddToChat={onAddToChat}
          onDismiss={dismissSelection}
        />
      ) : null}
    </div>
  );
}

function LiveWorking({
  startedAt,
  paused,
}: {
  startedAt?: number;
  paused: boolean;
}) {
  const elapsedMs = useElapsedFrom(startedAt, paused);
  return <TurnDuration elapsedMs={elapsedMs} live waiting={paused} />;
}

function TurnDuration({
  elapsedMs,
  live = false,
  done = false,
  waiting = false,
  showElapsed = true,
  completedAt,
  copyText: output,
  fromHarness,
  onSecondOpinion,
}: {
  elapsedMs: number | null;
  live?: boolean;
  done?: boolean;
  waiting?: boolean;
  showElapsed?: boolean;
  completedAt?: number;
  copyText?: string;
  fromHarness?: HarnessId;
  onSecondOpinion?: (harness: HarnessId, model: string) => void;
}) {
  const label = waiting
    ? "Waiting for approval"
    : formatWorkingDuration(elapsedMs, done);
  return (
    <div
      role={live ? "status" : undefined}
      aria-live={live ? "polite" : undefined}
      aria-label={
        waiting ? "Waiting for approval" : live ? "Agent is working" : label
      }
      className="flex items-center gap-3 px-4 pt-1 pb-3 font-sans text-sm text-content/40"
    >
      {done ? (
        <span className="flex items-center gap-2">
          {output ? (
            <CopyTurnButton text={output} />
          ) : (
            <Check className="size-3.5" strokeWidth={1.75} />
          )}
          {fromHarness && onSecondOpinion ? (
            <SecondOpinionButton from={fromHarness} onPick={onSecondOpinion} />
          ) : null}
        </span>
      ) : (
        <TerminalSpinner />
      )}

      {live && !done ? (
        <Shimmer duration={1}>{label}</Shimmer>
      ) : showElapsed ? (
        <span>{label}</span>
      ) : null}

      {completedAt != null ? (
        <span className="flex items-center gap-1 text-content/35">
          <Clock className="size-3.5 shrink-0" strokeWidth={1.75} />
          {formatClockTime(completedAt)}
        </span>
      ) : null}
    </div>
  );
}

/** Wall-clock stamp for a finished turn, in the reader's own locale. */
function formatClockTime(epochMs: number): string {
  return new Date(epochMs).toLocaleTimeString(undefined, {
    hour: "numeric",
    minute: "2-digit",
  });
}

function CopyTurnButton({ text }: { text: string }) {
  const [copied, setCopied] = useState(false);
  const timer = useRef<number | null>(null);

  useEffect(() => {
    setCopied(false);
    return () => {
      if (timer.current != null) window.clearTimeout(timer.current);
    };
  }, [text]);

  return (
    <button
      type="button"
      title={copied ? "Copied" : "Copy response"}
      aria-label={copied ? "Copied" : "Copy response"}
      className="-ml-1 rounded-md p-1 text-content/40 hover:bg-content/8 hover:text-content/70"
      onClick={() => {
        void copyText(text).then(
          () => {
            setCopied(true);
            if (timer.current != null) window.clearTimeout(timer.current);
            timer.current = window.setTimeout(() => setCopied(false), 2000);
          },
          () => {},
        );
      }}
    >
      {copied ? (
        <Check className="size-3.5" strokeWidth={1.75} />
      ) : (
        <Copy className="size-3.5" strokeWidth={1.75} />
      )}
    </button>
  );
}

const TranscriptBlock = memo(function TranscriptBlock({
  block,
  layout,
  stickyIndex,
  compactTop = false,
  cwd,
  onApproval,
  onOpenFile,
  onOpenDiff,
  onOpenPlan,
}: {
  block: Block;
  layout: TranscriptLayout;
  stickyIndex: number;
  compactTop?: boolean;
  cwd?: string;
  onApproval?: (requestId: number, decision: ApprovalDecision) => void;
  onOpenFile?: (path: string) => void;
  onOpenDiff?: (path: string) => void;
  onOpenPlan?: (blockId: string) => void;
}) {
  if (block.role === "user") {
    return (
      <UserMessageBlock
        block={block}
        layout={layout}
        stickyIndex={stickyIndex}
      />
    );
  }

  if (block.role === "tool") {
    return (
      <ToolCall
        block={block}
        cwd={cwd}
        onApproval={onApproval}
        onOpenFile={onOpenFile}
        onOpenDiff={onOpenDiff}
      />
    );
  }

  if (block.role === "reasoning") {
    return null;
  }

  if (block.role === "plan") {
    return (
      <div className="px-4 py-1">
        <PlanPreview
          text={block.text}
          streaming={block.streaming}
          onOpen={onOpenPlan ? () => onOpenPlan(block.id) : undefined}
        />
      </div>
    );
  }

  if (block.role === "approval") {
    return (
      <ToolCall
        block={block}
        cwd={cwd}
        onApproval={onApproval}
        onOpenFile={onOpenFile}
        onOpenDiff={onOpenDiff}
      />
    );
  }

  if (block.role === "handoff") {
    return <HandoffDivider block={block} />;
  }

  if (block.role === "system") {
    return (
      <div className="px-4 py-2 text-content/50">
        <pre className="min-w-0 whitespace-pre-wrap break-words">
          {block.text}
        </pre>
      </div>
    );
  }

  if (!block.text && block.streaming) return null;

  return (
    <div
      data-selectable-agent-response={block.streaming ? undefined : block.id}
      className={`min-w-0 px-4 pb-1 text-content ${compactTop ? "pt-2" : "pt-3"}`}
    >
      <AgentMarkdown
        text={block.text}
        streaming={block.streaming}
        cwd={cwd}
        onOpenFile={onOpenFile}
      />
    </div>
  );
});

function UserMessageBlock({
  block,
  layout,
  stickyIndex,
}: {
  block: Block;
  layout: TranscriptLayout;
  stickyIndex: number;
}) {
  const [expanded, setExpanded] = useState(false);
  const [overflows, setOverflows] = useState(false);
  const textRef = useRef<HTMLPreElement>(null);
  const card = block.secondOpinion;
  const text = card ? "" : block.text;
  const chat = layout === "chat";

  useLayoutEffect(() => {
    const el = textRef.current;
    if (!el || !text) {
      setOverflows(false);
      return;
    }
    if (expanded) return;
    setOverflows(el.scrollHeight > el.clientHeight + 1);
  }, [text, expanded]);

  const toggle = () => {
    if (overflows) setExpanded((value) => !value);
  };

  return (
    <div
      className={
        chat ? "flex justify-end pt-1.5 pr-4 pb-4 pl-14" : "p-1.5 pb-3"
      }
    >
      <div
        className={`min-w-0 rounded-lg border border-content/10 bg-content/10 px-3 py-2 font-sans text-content ${
          chat ? "w-fit max-w-xl" : ""
        }`}
        style={{ zIndex: stickyIndex }}
        onClick={overflows ? toggle : undefined}
      >
        {block.attachments?.length ? (
          <div
            className={`flex flex-wrap gap-1.5 ${text || card ? "mb-2" : ""}`}
          >
            {block.attachments.map((file) => (
              <AttachmentChip key={file.id} attachment={file} />
            ))}
          </div>
        ) : null}
        {card ? <SecondOpinionCard card={card} /> : null}
        {text ? (
          <pre
            ref={textRef}
            className={`min-w-0 whitespace-pre-wrap break-words font-sans text-sm ${expanded ? "" : "line-clamp-4"}`}
          >
            {text}
          </pre>
        ) : null}
      </div>
    </div>
  );
}

/** One activity row: py-1 around a 20px line. */
const ACTIVITY_ROW_HEIGHT = "h-7";

const DISCLOSURE_ROW = "flex w-fit items-center gap-1.5 py-1 font-sans text-sm";

function ActivityGroup({
  blocks,
  cwd,
  collapsed,
  zen,
  durationMs,
  startedAt,
  onApproval,
  onOpenFile,
}: {
  blocks: Block[];
  cwd?: string;
  collapsed?: boolean;
  zen?: boolean;
  durationMs?: number;
  startedAt?: number;
  onApproval?: (requestId: number, decision: ApprovalDecision) => void;
  onOpenFile?: (path: string) => void;
}) {
  // Live "+N previous" and the settled zen fold are independent. Sharing one
  // flag kept expanded history from folding when a turn settled.
  const [showPrevious, setShowPrevious] = useState(false);
  const [zenOpen, setZenOpen] = useState(false);
  const rolling = !!zen && !collapsed;
  const tickerIndex = useActivityTicker(
    blocks.filter((block) => !needsApproval(block)).length,
    rolling,
  );
  const { latest, pending, hidden } = splitActivityRows(blocks, tickerIndex);
  // Only the fold that carries the turn's clock ticks; everything else is
  // paused, so historical turns are not re-rendering once a second.
  const elapsedMs = useElapsedFrom(
    startedAt,
    durationMs != null || startedAt == null,
  );
  const view = activityGroupView(!!collapsed, pending.length, zenOpen);
  const previousCount = activityPreviousCount(hidden.length, !!latest, !!zen);
  const canRevealPrevious = hidden.length > 0;

  // Zen mode: once a turn settles the ticker folds into one line, and the whole
  // run — tool calls, notes, thinking — waits behind it.
  if (view === "summary" || view === "zen-expanded") {
    const open = view === "zen-expanded";
    const summary =
      durationMs != null
        ? formatWorkingDuration(durationMs, true)
        : startedAt != null
          ? formatWorkingDuration(elapsedMs, false)
          : activitySummary(blocks);
    return (
      <div className="flex min-w-0 flex-col gap-0 px-4">
        <button
          type="button"
          aria-expanded={open}
          aria-label={open ? "Hide the work" : `Show ${summary}`}
          onClick={() => setZenOpen(!open)}
          className={`${DISCLOSURE_ROW} text-content/40 transition-colors duration-200 hover:text-content/70`}
        >
          <ChevronRight
            className={`size-3.5 shrink-0 transition-transform duration-200 ${
              open ? "rotate-90" : ""
            }`}
            strokeWidth={1.75}
          />
          <span>{summary}</span>
        </button>
        {open ? (
          <div className="flex min-w-0 flex-col gap-0.5">
            {blocks.map((block) => (
              <ActivityRow
                key={block.id}
                block={block}
                cwd={cwd}
                expanded
                onApproval={onApproval}
                onOpenFile={onOpenFile}
              />
            ))}
          </div>
        ) : (
          <div aria-hidden className="pt-2">
            <div className="h-px w-full bg-content/10" />
          </div>
        )}
      </div>
    );
  }

  // Zen leaves the running turn as nothing but the ticker. The "+N previous"
  // row is on from the first step — not an empty spacer — so Working only
  // moves once, to sit under the step.
  return (
    <div className="flex min-w-0 flex-col gap-0.5 px-4">
      {previousCount > 0 ? (
        <button
          type="button"
          aria-expanded={canRevealPrevious ? showPrevious : undefined}
          aria-disabled={!canRevealPrevious}
          aria-label={
            showPrevious
              ? "Hide previous steps"
              : `Show ${previousCount} previous steps`
          }
          onClick={
            canRevealPrevious
              ? () => setShowPrevious((open) => !open)
              : undefined
          }
          className={`${DISCLOSURE_ROW} ${ACTIVITY_ROW_HEIGHT} shrink-0 text-content/40 transition-colors duration-200 ${
            canRevealPrevious ? "hover:text-content/70" : "cursor-default"
          }`}
        >
          <ChevronRight
            className={`size-3.5 shrink-0 transition-transform duration-200 ${
              showPrevious ? "rotate-90" : ""
            }`}
            strokeWidth={1.75}
          />
          <span>
            {showPrevious
              ? "Hide previous"
              : activityPreviousLabel(previousCount, !!zen)}
          </span>
        </button>
      ) : null}
      {showPrevious && canRevealPrevious
        ? hidden.map((block) => (
            <ActivityRow
              key={block.id}
              block={block}
              cwd={cwd}
              expanded
              onOpenFile={onOpenFile}
            />
          ))
        : null}
      {latest ? (
        <ActivityTicker
          block={latest}
          cwd={cwd}
          rolling={rolling}
          onOpenFile={onOpenFile}
        />
      ) : null}
      {pending.map((block) => (
        <ActivityRow
          key={block.id}
          block={block}
          cwd={cwd}
          onApproval={onApproval}
          onOpenFile={onOpenFile}
        />
      ))}
    </div>
  );
}

/**
 * The live line, as a ticker: the row the agent has moved on from rolls up out
 * of a one-line viewport while the new one rises into it. Zen only — without
 * it the row just swaps, the way it always has.
 */
function ActivityTicker({
  block,
  cwd,
  rolling,
  onOpenFile,
}: {
  block: Block;
  cwd?: string;
  rolling: boolean;
  onOpenFile?: (path: string) => void;
}) {
  const [state, setState] = useState<{
    current: Block;
    leaving: Block | null;
    roll: number;
  }>({ current: block, leaving: null, roll: 0 });

  // Adjusting during render keeps the swap in one frame: no flash of the old
  // row sitting in the new row's place.
  if (state.current.id !== block.id) {
    setState((prev) => ({
      current: block,
      leaving: rolling ? prev.current : null,
      roll: prev.roll + 1,
    }));
  } else if (state.current !== block) {
    setState((prev) => ({ ...prev, current: block }));
  }

  useEffect(() => {
    if (!state.leaving) return;
    const timer = window.setTimeout(
      () => setState((prev) => ({ ...prev, leaving: null })),
      TICKER_EXIT_MS,
    );
    return () => window.clearTimeout(timer);
  }, [state.leaving, state.roll]);

  if (!rolling) {
    return <ActivityRow block={block} cwd={cwd} onOpenFile={onOpenFile} />;
  }

  // The incoming row stays in flow so the viewport is exactly one row tall,
  // whatever that row turns out to be; only the outgoing one is lifted out.
  return (
    <div className="relative min-w-0 overflow-clip">
      {state.leaving ? (
        <div
          key={`out-${state.roll}`}
          aria-hidden
          className="zen-ticker-out pointer-events-none absolute inset-x-0 top-0"
        >
          <ActivityRow block={state.leaving} cwd={cwd} />
        </div>
      ) : null}
      <div
        key={`in-${state.roll}`}
        className={state.roll > 0 ? "zen-ticker-in" : undefined}
      >
        <ActivityRow
          block={state.current}
          cwd={cwd}
          live
          onOpenFile={onOpenFile}
        />
      </div>
    </div>
  );
}

/**
 * The live stack is a ticker: whatever the agent did last holds the line,
 * whether that was a tool call or a paragraph of prose.
 */
function ActivityRow({
  block,
  cwd,
  expanded = false,
  live = false,
  onApproval,
  onOpenFile,
}: {
  block: Block;
  cwd?: string;
  expanded?: boolean;
  live?: boolean;
  onApproval?: (requestId: number, decision: ApprovalDecision) => void;
  onOpenFile?: (path: string) => void;
}) {
  if (isThinkingBlock(block)) {
    return (
      <ActivityThinkingRow
        block={block}
        cwd={cwd}
        expandable={expanded}
        onOpenFile={onOpenFile}
      />
    );
  }
  if (isProseBlock(block)) {
    return expanded ? (
      <div className="flex min-w-0 gap-1.5 py-1 text-content">
        <Lightbulb
          className="mt-[5px] size-3.5 shrink-0 text-content/50"
          strokeWidth={1.75}
        />
        <div className="min-w-0 flex-1">
          <AgentMarkdown text={block.text} cwd={cwd} onOpenFile={onOpenFile} />
        </div>
      </div>
    ) : (
      <ActivityNoteRow block={block} />
    );
  }
  return (
    <ActivityToolRow
      block={block}
      cwd={cwd}
      live={live}
      onApproval={onApproval}
      onOpenFile={onOpenFile}
    />
  );
}

/**
 * The line that keeps a long think from reading as a stall. Opening the fold
 * around it does not open the thought itself — reasoning is only ever read on
 * purpose, one line until you ask for it.
 */
function ActivityThinkingRow({
  block,
  cwd,
  expandable = false,
  onOpenFile,
}: {
  block: Block;
  cwd?: string;
  expandable?: boolean;
  onOpenFile?: (path: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const text = proseSummary(block.text) || "Thinking";
  const icon = (
    <Brain
      className={`size-3.5 shrink-0 text-content/40 ${
        block.streaming ? "zen-thinking-pulse" : ""
      }`}
      strokeWidth={1.75}
    />
  );
  const label = (
    <span className="min-w-0 flex-1 truncate font-sans text-sm text-content/50">
      {text}
    </span>
  );

  if (!expandable) {
    return (
      <div
        aria-label={`Thinking: ${text}`}
        className="flex min-w-0 items-center gap-1.5 py-1"
      >
        {icon}
        {label}
      </div>
    );
  }

  return (
    <div className="flex min-w-0 flex-col">
      <button
        type="button"
        aria-expanded={open}
        aria-label={open ? "Hide thinking" : `Show thinking: ${text}`}
        onClick={() => setOpen((value) => !value)}
        className="group flex min-w-0 items-center gap-1.5 py-1 text-left"
      >
        {icon}
        <span className="min-w-0 flex-1 truncate font-sans text-sm text-content/50 transition-colors duration-200 group-hover:text-content/75">
          {text}
        </span>
      </button>
      {open ? (
        <div className="min-w-0 pb-2 pl-5">
          <AgentMarkdown
            className="agent-reasoning"
            text={block.text}
            cwd={cwd}
            onOpenFile={onOpenFile}
          />
        </div>
      ) : null}
    </div>
  );
}

function ActivityNoteRow({ block }: { block: Block }) {
  const text = proseSummary(block.text);
  return (
    <div
      aria-label={`Agent said: ${text}`}
      className="flex min-w-0 items-center gap-1.5 py-1"
    >
      <Lightbulb
        className="size-3.5 shrink-0 text-content/50"
        strokeWidth={1.75}
      />
      <span className="min-w-0 flex-1 truncate font-sans text-sm text-content/70">
        {text}
      </span>
    </div>
  );
}

function ActivityToolRow({
  block,
  cwd,
  live = false,
  onApproval,
  onOpenFile,
}: {
  block: Block;
  cwd?: string;
  live?: boolean;
  onApproval?: (requestId: number, decision: ApprovalDecision) => void;
  onOpenFile?: (path: string) => void;
}) {
  const label = toolCallLabel(block, cwd);
  const state = toolCallState(block);
  const pending = needsApproval(block);

  return (
    <div className="flex min-w-0 flex-col">
      <div
        aria-label={`Tool call: ${label}`}
        className="flex min-w-0 items-center gap-1.5 py-1"
      >
        <ActivityToolIcon block={block} state={state} live={live} />
        <ToolCallSummary
          label={label}
          preview={block.tool?.preview}
          cwd={cwd}
          onOpenFile={onOpenFile}
        />
        {pending ? null : <ToolCallStatusIcon state={state} />}
      </div>
      {pending ? (
        <ApprovalControls block={block} onApproval={onApproval} />
      ) : null}
    </div>
  );
}

function ActivityToolIcon({
  block,
  state,
  live = false,
}: {
  block: Block;
  state: ToolCallState;
  live?: boolean;
}) {
  if (state === "pending") {
    return (
      <CircleDashed
        className={`size-3.5 shrink-0 text-content/40 ${live ? "zen-ticker-live" : ""}`}
        strokeWidth={1.75}
      />
    );
  }

  const kind = block.tool?.preview?.kind ?? block.tool?.kind?.toLowerCase();
  const label = block.text || block.tool?.title || "";
  const className = "size-3.5 shrink-0 text-content/50";

  if (
    kind === "shell" ||
    /^(run|ran)\s+command/i.test(label) ||
    /shell|bash|execute/i.test(block.tool?.kind ?? "")
  ) {
    return <Terminal className={className} strokeWidth={1.75} />;
  }
  if (
    kind === "write" ||
    isEditTool(block.tool?.kind, label, block.tool?.preview)
  ) {
    return <Pencil className={className} strokeWidth={1.75} />;
  }
  if (
    kind === "read" ||
    isReadTool(block.tool?.kind, label, block.tool?.preview)
  ) {
    return <Hammer className={className} strokeWidth={1.75} />;
  }
  if (
    kind === "search" ||
    isSearchTool(block.tool?.kind, label, block.tool?.preview)
  ) {
    return <Search className={className} strokeWidth={1.75} />;
  }

  return <Hammer className={className} strokeWidth={1.75} />;
}

function ToolCallStatusIcon({ state }: { state: ToolCallState }) {
  const className = "size-3.5 shrink-0";
  if (state === "accepted") {
    return (
      <Check className={`${className} text-content/35`} strokeWidth={2.25} />
    );
  }
  if (state === "rejected") {
    return <X className={`${className} text-red-400`} strokeWidth={2} />;
  }
  return null;
}

function useElapsedFrom(
  startedAt: number | undefined,
  paused: boolean,
): number | null {
  const fallback = useRef<number | null>(null);
  const pausedMs = useRef(0);
  const pauseStarted = useRef<number | null>(null);
  const seenStartedAt = useRef(startedAt);

  if (seenStartedAt.current !== startedAt) {
    seenStartedAt.current = startedAt;
    fallback.current = null;
    pausedMs.current = 0;
    pauseStarted.current = paused ? Date.now() : null;
  }

  const origin = startedAt ?? (fallback.current ??= Date.now());
  const [elapsedMs, setElapsedMs] = useState(() =>
    Math.max(0, Date.now() - origin),
  );

  useEffect(() => {
    const start = startedAt ?? (fallback.current ??= Date.now());
    if (paused) {
      if (pauseStarted.current == null) pauseStarted.current = Date.now();
      return;
    }
    if (pauseStarted.current != null) {
      pausedMs.current += Date.now() - pauseStarted.current;
      pauseStarted.current = null;
    }
    const tick = () =>
      setElapsedMs(Math.max(0, Date.now() - start - pausedMs.current));
    tick();
    const id = window.setInterval(tick, 1000);
    return () => window.clearInterval(id);
  }, [startedAt, paused]);

  return elapsedMs;
}

function formatWorkingDuration(elapsedMs: number | null, done = false): string {
  if (elapsedMs == null) return done ? "Worked" : "Working…";
  const totalSec = Math.max(1, Math.round(elapsedMs / 1000));
  const label = done ? "Worked for" : "Working for";
  if (totalSec < 60) return `${label} ${totalSec}s`;
  const minutes = Math.floor(totalSec / 60);
  const seconds = totalSec % 60;
  return seconds ? `${label} ${minutes}m ${seconds}s` : `${label} ${minutes}m`;
}

function ToolCall({
  block,
  cwd,
  onApproval,
  onOpenFile,
  onOpenDiff,
  embedded,
}: {
  block: Block;
  cwd?: string;
  onApproval?: (requestId: number, decision: ApprovalDecision) => void;
  onOpenFile?: (path: string) => void;
  onOpenDiff?: (path: string) => void;
  embedded?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const preview = block.tool?.preview;
  const label = toolCallLabel(block, cwd);
  const detail = block.tool?.detail?.trim();
  const expanded = detail && detail !== label ? detail : label;
  const state = toolCallState(block);
  const stateLabel =
    state === "accepted"
      ? "Accepted"
      : state === "rejected"
        ? "Rejected"
        : "Pending";
  const editTool = isEditTool(
    block.tool?.kind,
    block.text || block.tool?.title,
    preview,
  );
  const compact =
    isReadTool(block.tool?.kind, label, preview) ||
    isSearchTool(block.tool?.kind, label, preview);
  const expandable = !compact && !!detail && detail !== label;

  const frame = embedded ? "py-0.5" : "px-4 py-1";

  if (editTool) {
    return (
      <div className={frame}>
        <FilePreview
          preview={preview ?? stubFilePreview(block.tool?.kind, label)}
          status={state}
          cwd={cwd}
          onOpenFile={onOpenDiff ?? onOpenFile}
        />
        <ApprovalControls block={block} onApproval={onApproval} />
      </div>
    );
  }

  if (isIncompleteTool(block, label, state)) return null;

  return (
    <div className={frame}>
      {expandable ? (
        <button
          type="button"
          aria-expanded={open}
          aria-label={`${stateLabel} tool call: ${label}`}
          onClick={() => setOpen((value) => !value)}
          className="flex w-full min-w-0 items-center gap-2 rounded-lg py-1.5 text-left"
        >
          <ToolCallIcon state={state} />
          <ToolCallSummary
            label={label}
            preview={preview}
            cwd={cwd}
            onOpenFile={onOpenFile}
          />
          <ChevronRight
            className={`size-3.5 shrink-0 text-content/35 transition-transform ${open ? "rotate-90" : ""}`}
            strokeWidth={1.75}
          />
        </button>
      ) : (
        <div
          aria-label={`${stateLabel} tool call: ${label}`}
          className="flex w-full min-w-0 items-center gap-2"
        >
          <ToolCallIcon state={state} />
          <ToolCallSummary
            label={label}
            preview={preview}
            cwd={cwd}
            onOpenFile={onOpenFile}
          />
        </div>
      )}
      {open && expandable ? (
        <pre className="mt-1.5 min-w-0 whitespace-pre-wrap break-words px-2.5 font-mono text-[12px] leading-5 text-content/55">
          {expanded}
        </pre>
      ) : null}
      <ApprovalControls block={block} onApproval={onApproval} />
    </div>
  );
}

function ToolCallSummary({
  label,
  preview,
  cwd,
  onOpenFile,
  interactive = true,
}: {
  label: string;
  preview?: ToolPreview;
  cwd?: string;
  onOpenFile?: (path: string) => void;
  interactive?: boolean;
}) {
  const parts = label.match(/^(Read|Find)\s+(.+)$/);
  // A write preview carries the path itself, so edits get the same verb + file
  // chip as reads rather than falling through to a raw label.
  const writeTarget =
    preview?.kind === "write"
      ? preview.path
        ? displayPath(preview.path, cwd)
        : preview.fileName
      : undefined;
  const action =
    parts?.[1] ??
    (writeTarget ? editVerb(label) : undefined) ??
    (/^read$/i.test(label.trim()) && (preview?.path || preview?.fileName)
      ? "Read"
      : /^find$/i.test(label.trim()) && preview?.query
        ? "Find"
        : undefined);
  const target =
    parts?.[2] ??
    writeTarget ??
    (action === "Read"
      ? preview?.path
        ? displayPath(preview.path, cwd)
        : preview?.fileName
      : action === "Find"
        ? preview?.query
        : undefined);
  if (!action || !target) {
    return (
      <span className="min-w-0 flex-1 truncate font-mono text-[13px] text-content/80">
        {label}
      </span>
    );
  }
  const isFile = action !== "Find";
  const fileName =
    preview?.fileName ||
    target
      .replace(/[/\\]+$/, "")
      .split(/[/\\]/)
      .filter(Boolean)
      .pop() ||
    "file";
  const filePath = resolveWorkspacePath(preview?.path || target, cwd);
  const canOpen = interactive && !!onOpenFile && !!filePath;

  return (
    <span className="flex min-w-0 flex-1 items-center gap-1.5 font-mono text-[13px]">
      <span className="shrink-0 text-content/50 font-sans text-sm">
        {action}
      </span>
      {isFile ? (
        canOpen ? (
          <button
            type="button"
            className="-my-0.5 flex min-w-0 flex-1 cursor-pointer items-center gap-1 rounded px-1 py-0.5 text-left text-content/85 hover:text-sky-300 hover:underline"
            title={preview?.path || target}
            onClick={(event) => {
              event.stopPropagation();
              onOpenFile?.(filePath);
            }}
          >
            <FileTypeIcon name={fileName} isDir={false} />
            <span className="min-w-0 truncate">{target}</span>
          </button>
        ) : (
          <span
            className="flex min-w-0 flex-1 items-center gap-1 px-1 text-content/85"
            title={preview?.path || target}
          >
            <FileTypeIcon name={fileName} isDir={false} />
            <span className="min-w-0 truncate">{target}</span>
          </span>
        )
      ) : (
        <span
          className="flex min-w-0 flex-1 items-center gap-1.5 text-content/85 pl-1"
          title={target}
        >
          <span className="min-w-0 truncate">{target}</span>
        </span>
      )}
    </span>
  );
}

function ToolCallIcon({ state }: { state: ToolCallState }) {
  const className = "size-3.5 shrink-0";
  if (state === "accepted") {
    return (
      <Check className={`${className} text-teal-400`} strokeWidth={2.25} />
    );
  }
  if (state === "rejected") {
    return <X className={`${className} text-red-400`} strokeWidth={2} />;
  }
  return (
    <CircleDashed
      className={`${className} text-content/40`}
      strokeWidth={1.75}
    />
  );
}

function ApprovalControls({
  block,
  onApproval,
}: {
  block: Block;
  onApproval?: (requestId: number, decision: ApprovalDecision) => void;
}) {
  const approval = block.approval;
  if (!approval || approval.decided) return null;
  return (
    <div className="mt-1.5 flex gap-2">
      <button
        type="button"
        className="rounded-md bg-content px-2.5 py-0.5 text-[11px] hover:bg-content/80     text-background-base"
        onClick={() => onApproval?.(approval.requestId, "allow")}
      >
        Allow
      </button>
      <button
        type="button"
        className="rounded-md bg-content/10 px-2.5 py-0.5 text-[11px] text-content/70 hover:bg-content/20"
        onClick={() => onApproval?.(approval.requestId, "deny")}
      >
        Deny
      </button>
    </div>
  );
}

function HandoffDivider({ block }: { block: Block }) {
  const meta = block.handoff;
  if (!meta) return null;

  const preparing = meta.status === "preparing";
  const label = preparing ? "Preparing a handoff" : HARNESS_TITLE[meta.to];

  return (
    <div className="px-4 py-5">
      <div className="flex items-center gap-3">
        <div className="h-px min-w-4 flex-1 bg-content/12" />
        <div
          role="separator"
          aria-label={
            preparing
              ? `Preparing a handoff to ${HARNESS_TITLE[meta.to]}`
              : `Continued with ${label}`
          }
          className="flex max-w-[min(100%,20rem)] items-center gap-1.5 px-1.5 font-sans text-[12px] text-content/55"
        >
          {preparing ? (
            <>
              <TerminalSpinner className="inline-block w-3.5 shrink-0 select-none text-center text-[11px] leading-none text-content/45" />
              <Shimmer duration={1.4}>{label}</Shimmer>
            </>
          ) : (
            <>
              <HarnessIcon harness={meta.to} className="size-3.5 shrink-0" />
            </>
          )}
        </div>
        <div className="h-px min-w-4 flex-1 bg-content/12" />
      </div>
    </div>
  );
}

function lastUserBlockId(blocks: Block[]): string | undefined {
  return turnUserBlock(blocks)?.id;
}

function turnUserBlock(blocks: Block[]): Block | undefined {
  for (let i = blocks.length - 1; i >= 0; i--) {
    if (blocks[i].role === "user") return blocks[i];
  }
  return undefined;
}

function isNearBottom(el: HTMLElement): boolean {
  return el.scrollHeight - el.scrollTop - el.clientHeight <= NEAR_BOTTOM_PX;
}

function pinToBottom(el: HTMLElement | null) {
  if (!el) return;
  el.scrollTop = el.scrollHeight;
}
