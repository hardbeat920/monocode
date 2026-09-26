import { uiScaleCommand } from "./uiScale";
import {
  keybindingPressed,
  matchCustomKeybinding,
  type ShortcutEvent,
} from "./settings";

export type ZoomAction = "zoom-in" | "zoom-out" | "zoom-reset";

const ZOOM_BY_COMMAND: Record<string, ZoomAction> = {
  "View: Zoom In": "zoom-in",
  "View: Zoom Out": "zoom-out",
  "View: Reset Zoom": "zoom-reset",
};

const ZOOM_BY_ACTION = new Map<string, string>(
  Object.entries(ZOOM_BY_COMMAND).map(([command, action]) => [action, command]),
);

type ZoomKeyEvent = ShortcutEvent & { isComposing: boolean; key: string };

/** The browser-standard chords, which the webview owns instead of the native menu. */
function defaultZoomAction(event: ZoomKeyEvent): ZoomAction | null {
  if (!event.metaKey && !event.ctrlKey) return null;
  if (event.altKey || event.isComposing) return null;
  return uiScaleCommand(event);
}

/**
 * Which zoom command a key press runs, or null. A rebound chord is resolved
 * first and may be Option-only, so it must not sit behind the Cmd/Ctrl guard
 * that only the browser-standard defaults need.
 */
export function resolveZoomKeybinding(event: ZoomKeyEvent): ZoomAction | null {
  const customCommand = event.isComposing ? null : matchCustomKeybinding(event);
  const customZoom = customCommand
    ? (ZOOM_BY_COMMAND[customCommand] ?? null)
    : null;
  const defaultZoom = customZoom ? null : defaultZoomAction(event);
  const zoom = customZoom ?? defaultZoom;
  if (!zoom) return null;
  const binding = ZOOM_BY_ACTION.get(zoom);
  return binding && keybindingPressed(binding, event, zoom === defaultZoom)
    ? zoom
    : null;
}
