/** Matches the composer's `max-h-40`, so the field stops growing where it clips. */
export const COMPOSER_MAX_HEIGHT = 160;

type Resizable = {
  style: { height: string };
  scrollHeight: number;
  parentElement?: {
    style: { minHeight: string };
    offsetHeight: number;
  } | null;
};

const measured = new WeakMap<HTMLTextAreaElement, string>();

function measurementKey(el: HTMLTextAreaElement): string {
  const style = getComputedStyle(el);
  return [
    el.value,
    el.clientWidth,
    style.font,
    style.lineHeight,
    style.letterSpacing,
    style.textIndent,
    style.padding,
    el.style.height,
  ].join("\0");
}

/** A warm tab already has the right height; don't collapse and reflow it twice. */
export function restoreComposerSize(el: HTMLTextAreaElement) {
  if (measured.get(el) === measurementKey(el)) return;
  resizeComposer(el);
}

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
  } finally {
    if (wrapper) wrapper.style.minHeight = minHeight;
  }
  if (
    typeof HTMLTextAreaElement !== "undefined" &&
    el instanceof HTMLTextAreaElement
  ) {
    measured.set(el, measurementKey(el));
  }
}
