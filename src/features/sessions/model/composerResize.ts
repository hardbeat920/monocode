/** Matches the composer's `max-h-40`, so the field stops growing where it clips. */
export const COMPOSER_MAX_HEIGHT = 160;

type Resizable = {
  style: { height: string; overflowY?: string };
  scrollHeight: number;
  parentElement?: {
    style: { minHeight: string };
    offsetHeight: number;
  } | null;
};

/** A hidden tab stays mounted with no layout box, so it reports 0 here. */
export function resizeComposer(el: Resizable, maxHeight = COMPOSER_MAX_HEIGHT) {
  if (el.scrollHeight === 0) return;
  const wrapper = el.parentElement;
  const minHeight = wrapper?.style.minHeight ?? "";
  // Measuring at `auto` briefly collapses the composer. Keep its space until
  // the final height is ready so the browser cannot clamp the transcript's
  // scroll position while its viewport temporarily grows.
  if (wrapper) wrapper.style.minHeight = `${wrapper.offsetHeight}px`;
  try {
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, maxHeight)}px`;
    // WebKit rounds scrollHeight up at non-100% zoom, so a field that fits
    // reports a 1px overflow; only scroll once it really passes the max.
    el.style.overflowY = el.scrollHeight > maxHeight ? "auto" : "hidden";
  } finally {
    if (wrapper) wrapper.style.minHeight = minHeight;
  }
}
