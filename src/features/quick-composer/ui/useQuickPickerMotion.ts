import { useLayoutEffect, useRef, type RefObject } from "react";
import { invoke } from "@tauri-apps/api/core";

const DURATION = 240;
const EASING = "cubic-bezier(0.22, 1, 0.36, 1)";

/** Animate the card itself so native vibrancy and its shadow follow its height. */
export function useQuickPickerMotion(
  frameRef: RefObject<HTMLDivElement | null>,
  pickerRef: RefObject<HTMLDivElement | null>,
  picker: "project" | "model" | "attachments" | "terminal" | null,
) {
  const lastHeight = useRef<number | null>(null);
  const previousPicker = useRef(picker);

  useLayoutEffect(() => {
    const frame = frameRef.current;
    if (!frame) return;
    let sentHeight = -1;
    const fit = () => {
      const height = frame.getBoundingClientRect().height;
      lastHeight.current = height;
      const rounded = Math.ceil(height);
      if (rounded === sentHeight) return;
      sentHeight = rounded;
      void invoke("quick_composer_fit", { height: rounded }).catch(
        () => undefined,
      );
    };
    fit();
    // ResizeObserver also runs on each animation frame, keeping the native
    // window's bottom edge in step with the card. The top edge stays fixed.
    const observer = new ResizeObserver(fit);
    observer.observe(frame);
    return () => observer.disconnect();
  }, [frameRef]);

  useLayoutEffect(() => {
    if (previousPicker.current === picker) return;
    previousPicker.current = picker;
    const frame = frameRef.current;
    if (!frame) return;
    const from = lastHeight.current;
    const to = frame.getBoundingClientRect().height;
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;

    const panel = pickerRef.current;
    // Reveal a stable layout as the card grows, rather than squeezing the
    // lists and their fixed headers into every intermediate height.
    if (panel) {
      panel.style.height = `${panel.getBoundingClientRect().height}px`;
      panel.style.flexShrink = "0";
    }
    const releasePanel = () => {
      if (!panel) return;
      panel.style.removeProperty("height");
      panel.style.removeProperty("flex-shrink");
    };
    const resize =
      from != null && Math.abs(from - to) > 0.5
        ? frame.animate([{ height: `${from}px` }, { height: `${to}px` }], {
            duration: DURATION,
            easing: EASING,
          })
        : null;
    const fade = panel?.animate([{ opacity: 0 }, { opacity: 1 }], {
      duration: 180,
      delay: 40,
      easing: "ease-out",
      fill: "backwards",
    });
    if (resize) resize.onfinish = releasePanel;
    else if (fade) fade.onfinish = releasePanel;
    else releasePanel();

    return () => {
      resize?.cancel();
      fade?.cancel();
      releasePanel();
    };
  }, [picker, frameRef, pickerRef]);
}
