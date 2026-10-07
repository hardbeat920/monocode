import { useCallback, useLayoutEffect, useRef, type RefObject } from "react";
import type { Block } from "../model/session";

const FOLLOW_TIME_MS = 85;
const SETTLED_PX = 0.25;
/**
 * How long growth keeps easing after a reply ends. Its last words are still
 * revealed at pace, and the turn's footer settles, after `busy` clears.
 */
const SETTLE_AFTER_REPLY_MS = 1200;

/**
 * Keep the real scroll position at the bottom, and ease the visible content
 * into it. The clipped wrapper keeps that temporary offset out of scrollHeight.
 * Following an existing turn also covers short chats, whose rows move upward
 * before the conversation is tall enough to scroll.
 */
export function useBottomChatMotion(
  scroller: HTMLDivElement | null,
  enabled: boolean,
  stickToBottom: RefObject<boolean>,
  blocks: readonly Block[],
  firstSend: boolean,
  animateGrowth = true,
  historicalBlockIds?: ReadonlySet<string>,
) {
  const latest = useRef({ blocks, firstSend, historicalBlockIds });
  latest.current = { blocks, firstSend, historicalBlockIds };
  const update = useRef<(() => void) | null>(null);
  const introduced = useRef(false);
  const growthUntil = useRef(animateGrowth ? Infinity : 0);

  // Declared before the measuring effect so the reply's end is known first.
  useLayoutEffect(() => {
    growthUntil.current = animateGrowth
      ? Infinity
      : growthUntil.current === Infinity
        ? performance.now() + SETTLE_AFTER_REPLY_MS
        : growthUntil.current;
  }, [animateGrowth]);

  useLayoutEffect(() => {
    const content = scroller?.querySelector<HTMLElement>(
      "[data-transcript-content]",
    );
    if (!enabled || !scroller || !content) return;

    const reducedMotion = window.matchMedia?.(
      "(prefers-reduced-motion: reduce)",
    );
    const seen = new Set(latest.current.blocks.map((block) => block.id));
    let scannedBlocks = latest.current.blocks;
    let scannedCount = scannedBlocks.length;
    const entering = new Set<string>();
    if (!introduced.current && latest.current.firstSend) {
      for (const block of latest.current.blocks) {
        if (
          block.role === "user" &&
          !block.draft &&
          !latest.current.historicalBlockIds?.has(block.id)
        )
          entering.add(block.id);
      }
    }
    introduced.current = true;
    const entrances = new Set<Animation>();
    let anchor: HTMLElement | null = null;
    let anchorTop = 0;
    let width = scroller.clientWidth;
    let height = scroller.clientHeight;
    let bottom = Math.max(0, scroller.scrollHeight - height);
    let wasFollowing = stickToBottom.current;
    let offset = 0;
    let frame = 0;
    let lastFrame = 0;

    // Content coordinates exclude native scrolling and movement of the pane.
    // Only a layout change may contribute to the eased message motion.
    const position = (element: HTMLElement) =>
      element.getBoundingClientRect().top -
      scroller.getBoundingClientRect().top +
      scroller.scrollTop -
      offset;

    const reset = () => {
      if (frame) cancelAnimationFrame(frame);
      frame = 0;
      offset = 0;
      if (content.style.transform) content.style.removeProperty("transform");
    };
    const tick = (now: number) => {
      frame = 0;
      if (!scroller.isConnected) {
        reset();
        return;
      }
      if (!stickToBottom.current) {
        pause();
        return;
      }
      const elapsed = Math.min(64, Math.max(0, now - lastFrame));
      lastFrame = now;
      offset *= Math.exp(-elapsed / FOLLOW_TIME_MS);
      if (Math.abs(offset) < SETTLED_PX) {
        reset();
        return;
      }
      content.style.transform = `translateY(${offset}px)`;
      frame = requestAnimationFrame(tick);
    };
    const measure = () => {
      if (!scroller.isConnected) return;
      const following = stickToBottom.current;
      if (!following) {
        pause();
      } else {
        scroller.scrollTop = scroller.scrollHeight;
        const resized =
          width !== scroller.clientWidth || height !== scroller.clientHeight;
        width = scroller.clientWidth;
        height = scroller.clientHeight;
        const nextBottom = Math.max(0, scroller.scrollHeight - height);
        const nextAnchor = content.lastElementChild as HTMLElement | null;
        // Read positions before changing the transform. Subtracting our current
        // offset lets the next streamed line continue the motion already in flight.
        const nextTop = nextAnchor ? position(nextAnchor) : 0;
        const shift =
          wasFollowing && anchor?.isConnected
            ? anchorTop - position(anchor) + nextBottom - bottom
            : 0;
        anchor = nextAnchor;
        anchorTop = nextTop;
        bottom = nextBottom;
        wasFollowing = true;

        const animated = performance.now() < growthUntil.current;
        if (resized || reducedMotion?.matches || !animated) {
          reset();
        } else {
          const limit = Math.max(0, height * 0.75);
          offset = Math.max(-limit, Math.min(limit, offset + shift));
          if (Math.abs(offset) >= SETTLED_PX) {
            content.style.transform = `translateY(${offset}px)`;
            if (!frame) {
              lastFrame = performance.now();
              frame = requestAnimationFrame(tick);
            }
          }
        }
      }

      // Markdown can reflow several times between transcript updates. Only
      // inspect message IDs when the blocks change, not on every resize.
      if (
        latest.current.blocks !== scannedBlocks ||
        latest.current.blocks.length !== scannedCount
      ) {
        scannedBlocks = latest.current.blocks;
        scannedCount = scannedBlocks.length;
        for (const block of scannedBlocks) {
          if (seen.has(block.id)) continue;
          seen.add(block.id);
          if (
            (block.role === "user" || block.role === "assistant") &&
            !block.draft &&
            !latest.current.historicalBlockIds?.has(block.id)
          )
            entering.add(block.id);
        }
      }
      if (!following || reducedMotion?.matches) {
        entering.clear();
        return;
      }
      if (!entering.size) return;
      for (const message of content.querySelectorAll<HTMLElement>(
        "[data-chat-message]",
      )) {
        const id = message.dataset.chatMessage!;
        if (!entering.delete(id)) continue;
        if (!message.animate) continue;
        const user = message.dataset.chatMessageRole === "user";
        const origin = user ? "100% 100%" : "0% 100%";
        const animation = message.animate(
          [
            {
              opacity: 0,
              transform: `translateY(${user ? 16 : 10}px) scale(${user ? 0.94 : 0.985})`,
              transformOrigin: origin,
            },
            {
              opacity: 1,
              transform: "translateY(0) scale(1)",
              transformOrigin: origin,
            },
          ],
          {
            duration: user ? 360 : 280,
            easing: "cubic-bezier(0.22, 1, 0.36, 1)",
          },
        );
        entrances.add(animation);
        animation.onfinish = animation.oncancel = () =>
          entrances.delete(animation);
      }
    };
    const stop = () => {
      reset();
      for (const animation of entrances) animation.cancel();
      entrances.clear();
    };
    const pause = () => {
      const visibleTop = offset ? scroller.scrollTop - offset : null;
      stickToBottom.current = false;
      wasFollowing = false;
      stop();
      // Hand the eased visual position to native scrolling without a jump.
      // Settled motion must not write scrollTop during the native gesture.
      if (visibleTop !== null) scroller.scrollTop = Math.max(0, visibleTop);
    };
    const onWheel = (event: WheelEvent) => {
      if (event.deltaY < 0) pause();
    };
    const onScroll = () => {
      if (!stickToBottom.current && offset) pause();
    };
    const onKeyDown = (event: KeyboardEvent) => {
      if (["ArrowUp", "PageUp", "Home"].includes(event.key)) pause();
    };
    const observer = new ResizeObserver(measure);
    observer.observe(content);
    observer.observe(scroller);
    scroller.addEventListener("wheel", onWheel, { passive: true });
    scroller.addEventListener("scroll", onScroll, { passive: true });
    scroller.addEventListener("touchstart", pause, { passive: true });
    scroller.addEventListener("keydown", onKeyDown);
    reducedMotion?.addEventListener?.("change", stop);
    update.current = measure;
    return () => {
      update.current = null;
      observer.disconnect();
      scroller.removeEventListener("wheel", onWheel);
      scroller.removeEventListener("scroll", onScroll);
      scroller.removeEventListener("touchstart", pause);
      scroller.removeEventListener("keydown", onKeyDown);
      reducedMotion?.removeEventListener?.("change", stop);
      stop();
    };
  }, [scroller, enabled, stickToBottom]);

  // Run after the transcript's pin/restore effects, before this commit paints.
  useLayoutEffect(() => update.current?.());

  // Rejoining the end needs a fresh baseline even when only the jump button
  // updates. Keep the next arriving line animated without rerendering the chat.
  return useCallback(() => update.current?.(), [update]);
}
