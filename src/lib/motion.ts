/** Read the shared CSS tokens so pointer gestures and CSS use the same timing. */
export function reorderMotion() {
  const style = window.getComputedStyle(document.documentElement);
  const duration = style.getPropertyValue("--motion-reorder-duration").trim();
  const milliseconds =
    parseFloat(duration) * (duration.endsWith("ms") ? 1 : 1000);
  return {
    duration: Number.isFinite(milliseconds) ? Math.max(0, milliseconds) : 0,
    easing: style.getPropertyValue("--motion-ease-out").trim() || "linear",
  };
}
