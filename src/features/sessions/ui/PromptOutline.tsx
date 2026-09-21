import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type FocusEvent as ReactFocusEvent,
  type KeyboardEvent as ReactKeyboardEvent,
  type RefObject,
} from "react";
import {
  activePromptId,
  barLift,
  barWindow,
  promptBlocks,
  promptLabel,
  promptPreview,
  NEAR_END_PX,
  RIPPLE_SPAN,
  type OutlineAnchor,
  type OutlineBand,
} from "../model/promptOutline";
import type { Block } from "../model/session";
import { Popover } from "../../../shared/ui/Popover";

const OPEN_DELAY_MS = 25;
const SCROLL_INSET_PX = 8;
const POPOVER_WIDTH = 288;
const MIN_PROMPTS = 2;
const BAR_HEIGHT_PX = 2;
const BAR_WIDTH_PX = 11;
const BAR_WIDTH_LIFTED_PX = 24;
const BAR_OPACITY_IDLE = 0.15;
const BAR_OPACITY_LIT = 0.85;
const RIPPLE_STEP_MS = 18;
const BAR_GAP_PX = 10;
const BAR_GAP_MIN_PX = 1;
const BAR_STACK_MAX_PX = 330;
const BAR_STACK_PANE_SHARE = 0.75;
const SCROLLER = ".agent-transcript";
const TURN = ".transcript-turn";
const ANCHOR = "[data-prompt-anchor]";

type Hover = { id: string; el: HTMLElement };

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
  const [hover, setHover] = useState<Hover | null>(null);
  const [open, setOpen] = useState(false);
  const [focusId, setFocusId] = useState<string | null>(null);
  const rail = useRef<HTMLDivElement>(null);
  const frame = useRef<number | null>(null);
  const openTimer = useRef<number | null>(null);
  const pointerInside = useRef(false);
  const lastPromptId = useRef<string | null>(null);
  lastPromptId.current = prompts[prompts.length - 1]?.id ?? null;

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
    // Streaming re-measures on every frame, and it almost always lands here:
    // pinned to the end, where the last prompt wins whatever the anchors say.
    // Answer from the block list and skip the walk.
    const distanceToEnd =
      scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight;
    if (distanceToEnd <= NEAR_END_PX) {
      setActiveId(lastPromptId.current);
      return;
    }
    const anchors: OutlineAnchor[] = [];
    for (const el of scroller.querySelectorAll<HTMLElement>(ANCHOR)) {
      const id = el.dataset.promptAnchor;
      if (id) anchors.push({ id, ...promptBand(el, viewport) });
    }
    setActiveId(
      activePromptId(
        { top: viewport.top, bottom: viewport.bottom },
        anchors,
        distanceToEnd,
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
  useEffect(() => cancelOpen, []);

  /** The ripple follows the pointer at once. The card waits out a pass-through. */
  const hoverBar = (id: string, el: HTMLElement) => {
    setHover({ id, el });
    if (open || openTimer.current != null) return;
    openTimer.current = window.setTimeout(() => {
      openTimer.current = null;
      setOpen(true);
    }, OPEN_DELAY_MS);
  };
  const showBar = (id: string, el: HTMLElement) => {
    cancelOpen();
    setHover({ id, el });
    setOpen(true);
  };
  const close = () => {
    cancelOpen();
    setHover(null);
    setOpen(false);
  };

  const leaveRail = () => {
    pointerInside.current = false;
    // Keyboard focus holds the card open after the pointer moves away.
    if (keyboardFocused(rail.current)) return;
    close();
  };
  const blurRail = (event: ReactFocusEvent<HTMLDivElement>) => {
    if (event.currentTarget.contains(event.relatedTarget)) return;
    if (pointerInside.current) return;
    close();
  };

  const hoverId = hover?.id ?? null;
  const preview = useMemo(
    () => (hoverId ? promptPreview(blocks, hoverId) : null),
    [blocks, hoverId],
  );

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

  if (prompts.length < MIN_PROMPTS) return null;

  const activeIndex = prompts.findIndex((prompt) => prompt.id === activeId);
  const stack = barStack(
    prompts.length,
    activeIndex >= 0 ? activeIndex : null,
    stackBudget,
  );
  const bars = prompts.slice(stack.start, stack.end);
  const hoverIndex = hover ? bars.findIndex((bar) => bar.id === hover.id) : -1;
  // One tab stop for the whole rail. Arrow keys walk it from there.
  const tabId =
    [focusId, activeId].find((id) => bars.some((bar) => bar.id === id)) ??
    bars[0].id;

  const onKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    const step =
      event.key === "ArrowDown" ? 1 : event.key === "ArrowUp" ? -1 : 0;
    if (step === 0) return;
    event.preventDefault();
    const from = bars.findIndex((bar) => bar.id === tabId);
    const next = bars[from + step];
    if (!next) return;
    setFocusId(next.id);
    rail.current
      ?.querySelector<HTMLElement>(`[data-prompt-bar="${CSS.escape(next.id)}"]`)
      ?.focus();
  };

  return (
    <div
      ref={rail}
      role="toolbar"
      aria-label="Prompts"
      aria-orientation="vertical"
      style={{ width: BAR_WIDTH_LIFTED_PX }}
      onMouseEnter={() => {
        pointerInside.current = true;
      }}
      onMouseLeave={leaveRail}
      onBlur={blurRail}
      onKeyDown={onKeyDown}
      className="absolute top-1/2 right-4 z-30 flex -translate-y-1/2 flex-col items-end @max-[58rem]:hidden"
    >
      {bars.map((prompt, index) => {
        const lift = barLift(index, hoverIndex);
        const distance = hoverIndex < 0 ? 0 : Math.abs(index - hoverIndex);
        // The pointer owns the fill while it is on the rail. Off the rail, the
        // fill goes back to marking the scroll position.
        const lit =
          hoverIndex >= 0 ? index === hoverIndex : prompt.id === activeId;
        return (
          <button
            key={prompt.id}
            type="button"
            data-prompt-bar={prompt.id}
            tabIndex={prompt.id === tabId ? 0 : -1}
            aria-label={promptLabel(prompt)}
            aria-current={prompt.id === activeId ? "true" : undefined}
            onMouseEnter={(event) => hoverBar(prompt.id, event.currentTarget)}
            onFocus={(event) => {
              setFocusId(prompt.id);
              // A click focuses the bar too, and the pointer already opened
              // the card. Only arrow keys and Tab open it from here.
              if (event.currentTarget.matches(":focus-visible")) {
                showBar(prompt.id, event.currentTarget);
              }
            }}
            onClick={() => jumpTo(prompt.id)}
            style={{ height: BAR_HEIGHT_PX + stack.gap }}
            className="flex w-full shrink-0 items-center justify-end outline-none"
          >
            <span
              aria-hidden="true"
              style={{
                height: BAR_HEIGHT_PX,
                width:
                  BAR_WIDTH_PX + (BAR_WIDTH_LIFTED_PX - BAR_WIDTH_PX) * lift,
                opacity: lit ? BAR_OPACITY_LIT : BAR_OPACITY_IDLE,
                // The wave reaches the outer bars a beat after the hovered one.
                transitionDelay: `${Math.min(distance, RIPPLE_SPAN) * RIPPLE_STEP_MS}ms`,
              }}
              className="rounded-full bg-content transition-[width,opacity] duration-200 ease-out"
            />
          </button>
        );
      })}
      {open && preview && hoverIndex >= 0 ? (
        <Popover
          anchor={hover?.el ?? null}
          side="left"
          align="center"
          gap={10}
          width={POPOVER_WIDTH}
          onDismiss={close}
          aria-label="Prompt preview"
          className="pointer-events-none flex flex-col gap-1.5 p-3 font-sans"
        >
          <p className="line-clamp-2 text-sm leading-snug text-content">
            {preview.title}
          </p>
          {preview.reply ? (
            <p className="line-clamp-2 text-sm leading-snug text-content/45">
              {preview.reply}
            </p>
          ) : null}
          {preview.detail ? (
            <p className="line-clamp-2 border-l-2 border-content/15 pl-3 text-sm leading-snug text-content/35">
              {preview.detail}
            </p>
          ) : null}
        </Popover>
      ) : null}
    </div>
  );
}

/** Clicking a bar focuses it as well. Only a keyboard focus holds the card open. */
function keyboardFocused(rail: HTMLElement | null): boolean {
  const el = document.activeElement;
  return (
    el instanceof HTMLElement &&
    !!rail?.contains(el) &&
    el.matches(":focus-visible")
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
