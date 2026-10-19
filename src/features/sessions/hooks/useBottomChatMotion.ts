import { useCallback, useLayoutEffect, useRef, type RefObject } from "react";
import type { Block } from "../model/session";
import { innerScrollerTakes } from "../../../shared/hooks/useLockOverscroll";

const FOLLOW_TIME_MS = 85;
const SETTLED_PX = 0.25;
/*
 * Following streamed growth is a velocity spring with a feed-forward term
 * Transcript updates land in bursts; easing each one out on its own moves the
 * reply in jolts. The spring instead learns how fast the reply is growing and
 * travels at that speed, so a steady stream reads as one continuous glide.
 * Units are px per 60fps frame.
 */
/** Velocity kept from one frame to the next (higher glides longer). */
const SPRING_DAMPING = 0.7;
/** Pull toward the end (higher is snappier). */
const SPRING_STIFFNESS = 0.05;
/** Inertia (higher is slower to start and stop). */
const SPRING_MASS = 1.25;
const SPRING_FRAME_MS = 1000 / 60;
/** Rate the growth estimate follows the observed growth. */
const SPRING_GROWTH_EMA = 0.12;
/**
 * While the reply grows, trail the end by up to this much rather than hug a
 * moving edge. The transcript's bottom padding covers it.
 */
const SPRING_MAX_LEAD_PX = 32;
/** Keep the spring's state this long after it lands, so a pause in the stream resumes at speed. */
const SPRING_SETTLE_GRACE_MS = 500;
/** Longest stretch of time one growth sample is spread over. */
const SPRING_MAX_SAMPLE_FRAMES = 60;
/** Mono replies can appear together; keep their glide below 1080px/second. */
const CHAT_FOLLOW_MAX_SPEED = 18;

export type FollowSpring = { velocity: number; growth: number };

export function followSpring(): FollowSpring {
  return { velocity: 0, growth: 0 };
}

/**
 * Advance the spring over `frames` 60fps frames and return the distance still
 * left to travel. `offset` is how far the content trails the end, `grew` how
 * much the end moved over the last `sampleFrames`. Never overshoots the end.
 */
export function stepFollowSpring(
  spring: FollowSpring,
  offset: number,
  grew: number,
  frames: number,
  sampleFrames = frames,
  maxSpeed = Infinity,
): number {
  if (grew < -1) spring.growth = 0;
  else {
    const observed = Math.min(
      maxSpeed,
      Math.max(0, grew) / Math.max(0.25, sampleFrames),
    );
    spring.growth += SPRING_GROWTH_EMA * (observed - spring.growth);
  }
  const lead = Math.min(spring.growth * 9, SPRING_MAX_LEAD_PX);
  let velocity = spring.velocity;
  while (frames > 0 && offset > 0) {
    const h = Math.min(1, frames);
    frames -= h;
    const diff = Math.max(0, offset - lead);
    velocity +=
      h *
      ((SPRING_DAMPING * velocity + SPRING_STIFFNESS * diff) / SPRING_MASS -
        velocity);
    velocity = Math.min(velocity, maxSpeed - spring.growth);
    offset = Math.max(0, offset - (velocity + spring.growth) * h);
  }
  spring.velocity = velocity;
  return offset;
}

function springIdle(spring: FollowSpring): boolean {
  return spring.velocity < 0.05 && spring.growth < 0.05;
}
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
  /** Animate new messages in, as the chat layout does. */
  animateEntrances = true,
  /**
   * Until this time a wheel gesture of unknown direction may be scrolling
   * off the main thread; pinning would snap it back to the end.
   */
  holdUntil?: RefObject<number>,
  /** Preserve the full movement and bound its speed for whole Mono replies. */
  smoothReveals = false,
  /** Acknowledge native offsets written by the animation, including handoff. */
  onScrollAdjust?: (scroller: HTMLDivElement) => void,
) {
  const latest = useRef({
    blocks,
    firstSend,
    historicalBlockIds,
    animateEntrances,
  });
  latest.current = { blocks, firstSend, historicalBlockIds, animateEntrances };
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
    let spring = followSpring();
    // Growth since the spring last stepped, the feed-forward's sample.
    let grown = 0;
    let lastStep = 0;
    let settledAt: number | null = null;

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
      spring = followSpring();
      grown = 0;
      settledAt = null;
      if (content.style.transform) content.style.removeProperty("transform");
    };
    // Landed: stop drawing frames, but remember how fast the reply was going.
    const settle = (now: number) => {
      const kept = spring;
      reset();
      spring = kept;
      settledAt = now;
      lastStep = now;
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
      // Growth rides the spring. Shrinking (a fold closing at the end) has
      // nothing to track and just eases out.
      if (offset > 0) {
        offset = stepFollowSpring(
          spring,
          offset,
          grown,
          elapsed / SPRING_FRAME_MS,
          Math.min(
            SPRING_MAX_SAMPLE_FRAMES,
            Math.max(0, now - lastStep) / SPRING_FRAME_MS,
          ),
          smoothReveals ? CHAT_FOLLOW_MAX_SPEED : Infinity,
        );
      } else {
        offset *= Math.exp(-elapsed / FOLLOW_TIME_MS);
      }
      grown = 0;
      lastStep = now;
      if (Math.abs(offset) < SETTLED_PX) {
        if (offset > 0 || !springIdle(spring)) settle(now);
        else reset();
        return;
      }
      content.style.transform = `translateY(${offset}px)`;
      frame = requestAnimationFrame(tick);
    };
    const measure = () => {
      if (!scroller.isConnected) return;
      // Markdown can reflow several times between transcript updates. Only
      // inspect message IDs when the blocks change, not on every resize.
      let sentPrompt = false;
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
          ) {
            entering.add(block.id);
            if (block.role === "user") sentPrompt = true;
          }
        }
      }
      const { animateEntrances } = latest.current;
      const following = stickToBottom.current;
      if (following && performance.now() < (holdUntil?.current ?? 0)) return;
      if (!following) {
        pause();
      } else {
        scroller.scrollTop = scroller.scrollHeight;
        onScrollAdjust?.(scroller);
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
        // Without entrances the transcript brings a sent prompt in itself.
        if (
          resized ||
          reducedMotion?.matches ||
          !animated ||
          (!animateEntrances && sentPrompt)
        ) {
          reset();
        } else {
          const limit = Math.max(0, height * 0.75);
          // Capping a whole reply's offset skips the rest of its movement
          // immediately. Mono keeps that distance in the bounded glide.
          offset = Math.max(
            -limit,
            smoothReveals ? offset + shift : Math.min(limit, offset + shift),
          );
          grown += shift;
          if (Math.abs(offset) >= SETTLED_PX) {
            content.style.transform = `translateY(${offset}px)`;
            if (!frame) {
              const now = performance.now();
              if (
                settledAt === null ||
                now - settledAt > SPRING_SETTLE_GRACE_MS
              ) {
                spring = followSpring();
                lastStep = now;
              }
              settledAt = null;
              lastFrame = now;
              frame = requestAnimationFrame(tick);
            }
          }
        }
      }

      if (!following || reducedMotion?.matches || !animateEntrances) {
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
      if (visibleTop !== null) {
        scroller.scrollTop = Math.max(0, visibleTop);
        onScrollAdjust?.(scroller);
      }
    };
    const onWheel = (event: WheelEvent) => {
      // A nested code block scrolling up leaves the reader at the end.
      if (event.deltaY < 0 && !innerScrollerTakes(scroller, event)) pause();
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
  }, [
    scroller,
    enabled,
    stickToBottom,
    holdUntil,
    smoothReveals,
    onScrollAdjust,
  ]);

  // Run after the transcript's pin/restore effects, before this commit paints.
  useLayoutEffect(() => update.current?.());

  // Rejoining the end needs a fresh baseline even when only the jump button
  // updates. Keep the next arriving line animated without rerendering the chat.
  return useCallback(() => update.current?.(), [update]);
}
