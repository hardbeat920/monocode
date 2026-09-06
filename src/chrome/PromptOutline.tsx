import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type MouseEvent as ReactMouseEvent,
  type RefObject,
} from "react";
import {
  activePromptId,
  barWindow,
  promptBlocks,
  promptLabel,
  type OutlineAnchor,
  type OutlineBand,
} from "../lib/promptOutline";
import type { Block } from "../lib/session";
import { Popover } from "./Popover";

const OPEN_DELAY_MS = 25;
const CLOSE_DELAY_MS = 100;
const SCROLL_INSET_PX = 8;
const POPOVER_WIDTH = 288;
const POPOVER_MAX_HEIGHT = 360;
const MIN_PROMPTS = 2;
const BAR_HEIGHT_PX = 2;
const BAR_GAP_PX = 5;
const BAR_GAP_MIN_PX = 1;
const BAR_STACK_MAX_PX = 330;
const BAR_STACK_PANE_SHARE = 0.75;
const SCROLLER = ".agent-transcript";
const TURN = ".transcript-turn";
const ANCHOR = "[data-prompt-anchor]";

type Props = {
  blocks: Block[];
  scope: RefObject<HTMLElement | null>;
  visible?: boolean;
  /** Renders the turn that holds the block. Returns false when the block is unknown. */
  revealBlock?: (blockId: string) => boolean;
};

export function PromptOutline({
  blocks,
  scope,
  visible = true,
  revealBlock,
}: Props) {
  const prompts = useMemo(() => promptBlocks(blocks), [blocks]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [stackBudget, setStackBudget] = useState(BAR_STACK_MAX_PX);
  const [open, setOpen] = useState(false);
  const [point, setPoint] = useState<{ x: number; y: number } | null>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const list = useRef<HTMLDivElement>(null);
  const activeRow = useRef<HTMLButtonElement>(null);
  const frame = useRef<number | null>(null);
  const openTimer = useRef<number | null>(null);
  const closeTimer = useRef<number | null>(null);
  const reopenBlockedUntilLeave = useRef(false);

  const measure = useCallback(() => {
    const scroller = scope.current?.querySelector<HTMLElement>(SCROLLER);
    if (!scroller) {
      setActiveId(null);
      return;
    }
    const viewport = scroller.getBoundingClientRect();
    // A hidden tab has zero-size boxes. The rule would then select the last prompt.
    if (viewport.height === 0) return;
    setStackBudget(
      Math.min(
        BAR_STACK_MAX_PX,
        Math.floor(viewport.height * BAR_STACK_PANE_SHARE),
      ),
    );
    const anchors: OutlineAnchor[] = [];
    for (const el of scroller.querySelectorAll<HTMLElement>(ANCHOR)) {
      const id = el.dataset.promptAnchor;
      if (id) anchors.push({ id, ...promptBand(el, viewport) });
    }
    setActiveId(
      activePromptId(
        { top: viewport.top, bottom: viewport.bottom },
        anchors,
        scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight,
      ),
    );
  }, [scope]);

  const schedule = useCallback(() => {
    if (frame.current != null) return;
    frame.current = window.requestAnimationFrame(() => {
      frame.current = null;
      measure();
    });
  }, [measure]);

  useEffect(() => {
    const scroller = scope.current?.querySelector<HTMLElement>(SCROLLER);
    if (!scroller) return;
    scroller.addEventListener("scroll", schedule, { passive: true });
    const observer = new ResizeObserver(schedule);
    observer.observe(scroller);
    // Content growth moves the anchors without a scroll event.
    if (scroller.firstElementChild)
      observer.observe(scroller.firstElementChild);
    schedule();
    return () => {
      scroller.removeEventListener("scroll", schedule);
      observer.disconnect();
      if (frame.current != null) {
        window.cancelAnimationFrame(frame.current);
        frame.current = null;
      }
    };
  }, [schedule, scope]);

  useEffect(() => {
    schedule();
  }, [schedule, blocks, visible]);

  const cancelOpen = () => {
    if (openTimer.current == null) return;
    window.clearTimeout(openTimer.current);
    openTimer.current = null;
  };
  const cancelClose = () => {
    if (closeTimer.current == null) return;
    window.clearTimeout(closeTimer.current);
    closeTimer.current = null;
  };
  useEffect(
    () => () => {
      cancelOpen();
      cancelClose();
    },
    [],
  );

  const openNow = () => {
    cancelOpen();
    cancelClose();
    const rect = trigger.current?.getBoundingClientRect();
    if (!rect) return;
    // Anchor the list at the top-right corner of the trigger. The list covers
    // the trigger, so the pointer crosses no gap.
    setPoint({ x: rect.right, y: rect.top });
    setOpen(true);
  };
  const closeNow = () => {
    cancelOpen();
    cancelClose();
    setOpen(false);
  };
  const scheduleClose = () => {
    cancelClose();
    closeTimer.current = window.setTimeout(() => {
      closeTimer.current = null;
      setOpen(false);
    }, CLOSE_DELAY_MS);
  };
  const enterTrigger = () => {
    cancelClose();
    if (open || reopenBlockedUntilLeave.current || openTimer.current != null) {
      return;
    }
    openTimer.current = window.setTimeout(() => {
      openTimer.current = null;
      openNow();
    }, OPEN_DELAY_MS);
  };
  const leaveTrigger = () => {
    reopenBlockedUntilLeave.current = false;
    cancelOpen();
    if (open) scheduleClose();
  };

  useEffect(() => {
    if (!open) return;
    // Wait one frame for placement. Then the list has its final height.
    const id = window.requestAnimationFrame(() => {
      const row = activeRow.current;
      const box = list.current;
      if (!row || !box) return;
      const top = row.offsetTop;
      const bottom = top + row.offsetHeight;
      if (top < box.scrollTop) box.scrollTop = top;
      else if (bottom > box.scrollTop + box.clientHeight) {
        box.scrollTop = bottom - box.clientHeight;
      }
    });
    return () => window.cancelAnimationFrame(id);
  }, [open]);

  const jumpTo = (id: string) => {
    const scroller = scope.current?.querySelector<HTMLElement>(SCROLLER);
    if (!scroller) return;
    // The transcript scrolls to the bottom on each streaming update until a
    // wheel-up event occurs. Send one, so the jump stays.
    scroller.dispatchEvent(new WheelEvent("wheel", { deltaY: -1 }));
    const selector = `[data-prompt-anchor="${CSS.escape(id)}"]`;
    let anchor = scroller.querySelector<HTMLElement>(selector);
    if (!anchor && revealBlock?.(id)) {
      anchor = scroller.querySelector<HTMLElement>(selector);
    }
    if (!anchor) {
      scroller.scrollTop = 0;
      return;
    }
    const target = anchor.closest<HTMLElement>(TURN) ?? anchor;
    const align = () => {
      const delta =
        target.getBoundingClientRect().top -
        scroller.getBoundingClientRect().top -
        SCROLL_INSET_PX;
      if (Math.abs(delta) > 2) scroller.scrollTop += delta;
    };
    align();
    // Turns that enter the screen get their real height. That can move the
    // target.
    window.requestAnimationFrame(align);
  };

  const onRowClick = (event: ReactMouseEvent, id: string) => {
    jumpTo(id);
    const rect = trigger.current?.getBoundingClientRect();
    reopenBlockedUntilLeave.current =
      !!rect &&
      event.clientX >= rect.left &&
      event.clientX <= rect.right &&
      event.clientY >= rect.top &&
      event.clientY <= rect.bottom;
    closeNow();
  };

  if (prompts.length < MIN_PROMPTS) return null;

  const activeIndex = prompts.findIndex((prompt) => prompt.id === activeId);
  const stack = barStack(
    prompts.length,
    activeIndex >= 0 ? activeIndex : null,
    stackBudget,
  );
  const bars = prompts.slice(stack.start, stack.end);

  return (
    <div
      className="absolute top-3 right-4 z-30"
      onMouseEnter={enterTrigger}
      onMouseLeave={leaveTrigger}
    >
      <button
        ref={trigger}
        type="button"
        aria-label="Prompts"
        aria-expanded={open}
        onClick={openNow}
        style={{ gap: stack.gap }}
        className="flex flex-col items-end rounded-lg p-1 hover:bg-content/6"
      >
        {bars.map((prompt) => (
          <span
            key={prompt.id}
            aria-hidden="true"
            style={{ height: BAR_HEIGHT_PX }}
            className={`w-5.5 rounded-full ${
              prompt.id === activeId ? "bg-content/85" : "bg-content/15"
            }`}
          />
        ))}
      </button>
      {open && point ? (
        <Popover
          anchor={point}
          side="left"
          align="start"
          gap={0}
          width={POPOVER_WIDTH}
          maxHeight={POPOVER_MAX_HEIGHT}
          onDismiss={closeNow}
          aria-label="Prompts"
          className="flex flex-col"
          onMouseEnter={cancelClose}
          onMouseLeave={scheduleClose}
        >
          <div
            ref={list}
            className="prompt-outline-list relative flex min-h-0 flex-1 flex-col gap-0.5 overflow-y-auto overscroll-none p-1"
          >
            {prompts.map((prompt) => {
              const active = prompt.id === activeId;
              return (
                <button
                  key={prompt.id}
                  ref={active ? activeRow : undefined}
                  type="button"
                  aria-current={active ? "true" : undefined}
                  onClick={(event) => onRowClick(event, prompt.id)}
                  className={`block w-full shrink-0 truncate rounded-lg px-3 py-1 text-left font-sans text-sm ${
                    active
                      ? "bg-content/10 text-content"
                      : "text-content/85 hover:bg-content/6"
                  }`}
                >
                  {promptLabel(prompt)}
                </button>
              );
            })}
          </div>
        </Popover>
      ) : null}
    </div>
  );
}

/**
 * content-visibility skips off-screen turns. A read inside a skipped turn
 * forces its layout. Use the turn box for an off-screen turn. Use the exact
 * prompt box for an on-screen turn.
 */
function promptBand(anchor: HTMLElement, viewport: DOMRect): OutlineBand {
  const turn = anchor.closest<HTMLElement>(TURN) ?? anchor;
  const turnBox = turn.getBoundingClientRect();
  const onScreen =
    turnBox.bottom > viewport.top && turnBox.top < viewport.bottom;
  const box =
    turn !== anchor && onScreen ? anchor.getBoundingClientRect() : turnBox;
  return { top: box.top, bottom: box.bottom };
}

/** One bar per prompt while the bars fit the budget. The gap shrinks first. Past that, a window slides. */
function barStack(count: number, activeIndex: number | null, budget: number) {
  const fit = Math.max(
    1,
    Math.floor((budget + BAR_GAP_MIN_PX) / (BAR_HEIGHT_PX + BAR_GAP_MIN_PX)),
  );
  const window_ = barWindow(count, activeIndex, fit);
  const shown = window_.end - window_.start;
  const gap =
    shown > 1
      ? Math.min(
          BAR_GAP_PX,
          Math.max(
            BAR_GAP_MIN_PX,
            Math.floor((budget - shown * BAR_HEIGHT_PX) / (shown - 1)),
          ),
        )
      : 0;
  return { ...window_, gap };
}
