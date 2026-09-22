import { IS_WIN } from "../../platform/tauri/platform";

/**
 * Convert a Tauri drag-drop position to CSS pixels. The API types it as
 * PhysicalPosition, but only WebView2 on Windows reports physical pixels.
 * macOS (NSDraggingInfo) and GTK report logical points already.
 */
export function dragPointToClient(
  x: number,
  y: number,
  isWin: boolean = IS_WIN,
  scale: number = window.devicePixelRatio || 1,
): { x: number; y: number } {
  if (!isWin || scale === 1) return { x, y };
  return { x: x / scale, y: y / scale };
}
