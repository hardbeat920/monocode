import { useLayoutEffect, useRef, type RefObject } from "react";
import { layoutTransitionActive } from "../../../shared/lib/layoutTransition";
import { innerScrollerTakes } from "../../../shared/hooks/useLockOverscroll";

type Anchor = {
  message: HTMLElement;
  character: number;
  top: number;
};

/** Keep the reader's character in view through file reflow and reply completion. */
export function useReplyReflowAnchor(
  scroller: HTMLDivElement | null,
  visible: boolean,
  following: RefObject<boolean>,
  remember: (element: HTMLElement) => void,
  adjustments: RefObject<number | undefined>,
) {
  // The cached transcript can detach while hidden, and its width or reply can
  // change before it returns. Keep the character as well as the scroll offset.
  const retained = useRef<Anchor | undefined>(undefined);
  useLayoutEffect(() => {
    if (!visible || !scroller) return;
    let width = scroller.clientWidth;
    let anchor = following.current ? undefined : retained.current;
    let frame = 0;
    // The activation effect restores the cached native offset before this
    // hook runs. Its queued scroll event must not replace the retained glyph
    // before the first resize delivery can restore that glyph's position.
    let adjustedTop: number | undefined = scroller.scrollTop;
    let measuredTop = scroller.scrollTop;
    let manualUntil = 0;
    let waitingForScroll = false;
    let draggingScroll = false;
    const capture = () => {
      if (following.current) {
        anchor = undefined;
        return;
      }
      // A pending width change must use the position from before reflow.
      if (!scroller.isConnected || scroller.clientWidth !== width) return;
      const viewport = scroller.getBoundingClientRect();
      // A point can fall between list items or paragraphs. Try the next
      // nearby line before giving up on a reader's visible text.
      for (const offsetY of [32, 48, 64]) {
        for (const fraction of [0.25, 0.5, 0.75]) {
          const range = document.caretRangeFromPoint?.(
            viewport.left + viewport.width * fraction,
            viewport.top + offsetY,
          );
          if (!range || range.startContainer.nodeType !== Node.TEXT_NODE)
            continue;
          const node = range.startContainer as Text;
          const message = node.parentElement?.closest<HTMLElement>(
            '[data-chat-message-role="assistant"]',
          );
          if (!message || !scroller.contains(message) || !node.length) continue;
          const offset = Math.min(range.startOffset, node.length - 1);
          // A space can have a rectangle now and collapse after wrapping.
          // Bookmark a painted character, or a later zero rectangle can send
          // the transcript scrolling repeatedly toward the wrong position.
          if (/\s/.test(node.data[offset])) continue;
          range.setStart(node, offset);
          range.setEnd(node, offset + 1);
          const rect = range.getBoundingClientRect();
          if (!rect.width || !rect.height) continue;
          const prefix = document.createRange();
          prefix.selectNodeContents(message);
          prefix.setEnd(node, offset);
          anchor = {
            message,
            character: prefix.toString().length,
            top: rect.top - viewport.top,
          };
          measuredTop = scroller.scrollTop;
          return;
        }
      }
    };
    const schedule = () => {
      if (!frame)
        frame = requestAnimationFrame(() => {
          frame = 0;
          capture();
        });
    };
    const onScroll = () => {
      const top = scroller.scrollTop;
      const programmatic = adjustments.current;
      adjustments.current = undefined;
      if (
        (adjustedTop != null && Math.abs(top - adjustedTop) < 1) ||
        (programmatic != null && Math.abs(top - programmatic) < 1)
      ) {
        adjustedTop = undefined;
        return;
      }
      adjustedTop = undefined;
      const now = performance.now();
      // Starting a pane slide can briefly clamp the native offset before
      // resize observers see its animated width. Keep the saved character
      // unless input identifies this scroll as the reader's own movement.
      if (
        anchor &&
        layoutTransitionActive() &&
        !draggingScroll &&
        !waitingForScroll &&
        now >= manualUntil
      )
        return;
      if (draggingScroll || waitingForScroll || now < manualUntil) {
        // Momentum can outlast the wheel event that began it. Keep the
        // gesture active until scrolling itself has been quiet, otherwise
        // completion restores a character from partway through that scroll.
        manualUntil = now + 180;
        waitingForScroll = false;
      }
      // A handoff from eased motion can precede the native wheel movement by
      // more than one quiet window. Every unacknowledged scroll refreshes the
      // bookmark, while explicit layout corrections retain its character.
      schedule();
    };
    const manualScroll = () => {
      manualUntil = performance.now() + 180;
      // The first native scroll can arrive after the wheel's quiet window.
      // Keep that input pending without treating later layout corrections as
      // a new gesture once the input has settled.
      waitingForScroll = true;
      schedule();
    };
    const onWheel = (event: WheelEvent) => {
      if (!innerScrollerTakes(scroller, event)) manualScroll();
    };
    const pointerDown = () => {
      draggingScroll = true;
    };
    const pointerUp = () => {
      draggingScroll = false;
    };
    const keyDown = (event: KeyboardEvent) => {
      if (
        [
          "ArrowUp",
          "ArrowDown",
          "PageUp",
          "PageDown",
          "Home",
          "End",
          " ",
        ].includes(event.key)
      )
        manualScroll();
    };
    const resize = new ResizeObserver(() => {
      const next = scroller.clientWidth;
      if (!next || !scroller.isConnected) return;
      const reflow = next !== width;
      if (reflow && anchor) {
        // A wheel's pending capture must not choose a different character
        // after the divider has already changed the line wrapping.
        manualUntil = 0;
        waitingForScroll = false;
        if (frame) cancelAnimationFrame(frame);
        frame = 0;
      } else if (
        (draggingScroll ||
          waitingForScroll ||
          performance.now() < manualUntil) &&
        (adjustments.current == null ||
          Math.abs(scroller.scrollTop - adjustments.current) > 0.5) &&
        Math.abs(scroller.scrollTop - measuredTop) > 0.5
      ) {
        // Only an offset change belongs to the gesture. Streaming can reflow
        // after the reader stops, while the input's quiet window is still open.
        capture();
        width = next;
        return;
      }
      if (!following.current && anchor?.message.isConnected) {
        // Markdown can merge text into a surviving node during word-fade
        // cleanup. Resolve the saved character even if that node stayed
        // connected: its former offset may now point at different text.
        let offset = anchor.character;
        const walker = document.createTreeWalker(
          anchor.message,
          NodeFilter.SHOW_TEXT,
        );
        let node: Text | null;
        while ((node = walker.nextNode() as Text | null)) {
          if (offset < node.length) break;
          offset -= node.length;
        }
        if (node && offset < node.length) {
          const range = document.createRange();
          range.setStart(node, offset);
          range.setEnd(node, offset + 1);
          const rect = range.getBoundingClientRect();
          if (!rect.width || !rect.height) anchor = undefined;
          else {
            const shift =
              rect.top - scroller.getBoundingClientRect().top - anchor.top;
            if (Math.abs(shift) > 0.5) {
              scroller.scrollTop += shift;
              adjustedTop = scroller.scrollTop;
              remember(scroller);
            }
          }
        }
      }
      width = next;
      measuredTop = scroller.scrollTop;
      // Other transcript observers can also adjust the offset in this resize
      // delivery. Their queued scroll event must preserve the same character
      // through subsequent reflows, even when our own shift was zero.
      adjustedTop = scroller.scrollTop;
      // Keep this character for the whole drag. Picking another word at the
      // same x coordinate after each reflow would gradually drift by lines.
      if (!anchor) capture();
    });
    resize.observe(scroller);
    // Completion can remove a status row inside the reply's own turn. The
    // whole-turn observer only corrects turns above the reader, so retain the
    // character here even when the viewport width itself has not changed.
    if (scroller.firstElementChild) resize.observe(scroller.firstElementChild);
    scroller.addEventListener("scroll", onScroll, { passive: true });
    scroller.addEventListener("wheel", onWheel, { passive: true });
    scroller.addEventListener("touchmove", manualScroll, { passive: true });
    scroller.addEventListener("pointerdown", pointerDown, { passive: true });
    scroller.addEventListener("keydown", keyDown);
    document.addEventListener("pointerup", pointerUp, { passive: true });
    if (!anchor) schedule();
    return () => {
      retained.current = anchor;
      resize.disconnect();
      scroller.removeEventListener("scroll", onScroll);
      scroller.removeEventListener("wheel", onWheel);
      scroller.removeEventListener("touchmove", manualScroll);
      scroller.removeEventListener("pointerdown", pointerDown);
      scroller.removeEventListener("keydown", keyDown);
      document.removeEventListener("pointerup", pointerUp);
      if (frame) cancelAnimationFrame(frame);
    };
  }, [scroller, visible, following, remember, adjustments]);
}
