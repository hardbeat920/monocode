import type { AgentModel } from "./models";

/** Matches Opus 5.5 across catalog IDs (`opus-5-5`), names (`Opus 5.5`), and dated native IDs. */
export function isOpus55Model(model: AgentModel): boolean {
  return [model.id, model.nativeId, model.name].some(
    (value) =>
      value != null && /(^|[^a-z0-9])opus[\s-]?5[.-]5(?![0-9.])/i.test(value),
  );
}

const MIN_STAGE_BELOW = 140;
const MAX_STAGE_ABOVE = 240;

/**
 * Pane-relative band for the welcome scene: the free space under a centered
 * composer, or a band just above a docked one that leaves no room below.
 */
export function opusStage(
  paneHeight: number,
  composer?: { top: number; bottom: number },
): { top: number; height: number } {
  if (!composer) {
    const top = Math.round(paneHeight * 0.55);
    return { top, height: paneHeight - top };
  }
  const below = paneHeight - composer.bottom;
  if (below >= MIN_STAGE_BELOW) return { top: composer.bottom, height: below };
  const height = Math.max(0, Math.min(MAX_STAGE_ABOVE, composer.top));
  return { top: composer.top - height, height };
}
