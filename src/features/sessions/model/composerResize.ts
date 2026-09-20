/** Matches the composer's `max-h-40`, so the field stops growing where it clips. */
export const COMPOSER_MAX_HEIGHT = 160;

type Resizable = {
  style: { height: string };
  scrollHeight: number;
};

/** A hidden tab stays mounted with no layout box, so it reports 0 here. */
export function resizeComposer(el: Resizable, maxHeight = COMPOSER_MAX_HEIGHT) {
  if (el.scrollHeight === 0) return;
  el.style.height = "auto";
  el.style.height = `${Math.min(el.scrollHeight, maxHeight)}px`;
}
