/**
 * Where MonoCode draws no menu of its own, the webview's context menu only
 * offers page commands: Reload, Back, Forward, and Inspect Element in
 * development builds. Reload lives in the View menu, so that menu is hidden.
 * Menus with real content stay native: text editing and selection (Cut, Copy,
 * Paste, Look Up, spelling), links, and media.
 */

/** Inputs without editable text get the page menu, like any other control. */
const NON_TEXT_INPUT_TYPES = new Set([
  "button",
  "checkbox",
  "color",
  "file",
  "hidden",
  "image",
  "radio",
  "range",
  "reset",
  "submit",
]);

/**
 * Targets whose native menu has more than page commands. CodeMirror and xterm
 * are listed by container because a right-click lands on their rendered
 * lines, not on the contenteditable or hidden textarea that owns the text.
 */
const NATIVE_MENU_TARGETS = [
  "textarea",
  '[contenteditable]:not([contenteditable="false"])',
  ".cm-editor",
  ".monocode-terminal",
  ".xterm",
  "a[href]",
  "img",
  "video",
].join(", ");

export type ContextMenuEventLike = Pick<
  MouseEvent,
  "target" | "defaultPrevented" | "clientX" | "clientY" | "shiftKey"
>;

export type ContextMenuSelection = Pick<
  Selection,
  "isCollapsed" | "rangeCount" | "getRangeAt"
>;

function targetElement(target: EventTarget | null): Element | null {
  const node = target as Node | null;
  if (!node || typeof node.nodeType !== "number") return null;
  return node.nodeType === 1 ? (node as Element) : node.parentElement;
}

function hasNativeMenu(element: Element): boolean {
  if (element.closest(NATIVE_MENU_TARGETS)) return true;
  const input = element.closest("input");
  return Boolean(input && !NON_TEXT_INPUT_TYPES.has(input.type));
}

/**
 * WebKit offers Copy and Look Up only when the click lands on the selection
 * (on macOS it first selects the word under the pointer), so this follows the
 * same hit test instead of asking whether anything is selected at all.
 */
function isOnSelection(
  selection: ContextMenuSelection | null,
  x: number,
  y: number,
): boolean {
  if (!selection || selection.isCollapsed) return false;
  for (let i = 0; i < selection.rangeCount; i++) {
    const rects = selection.getRangeAt(i).getClientRects();
    for (let j = 0; j < rects.length; j++) {
      const rect = rects[j];
      if (
        x >= rect.left &&
        x <= rect.right &&
        y >= rect.top &&
        y <= rect.bottom
      ) {
        return true;
      }
    }
  }
  return false;
}

/**
 * Whether a `contextmenu` event would open a menu of page commands only.
 * `allowInspect` keeps Shift+right-click native so development builds can
 * still reach Inspect Element.
 */
export function shouldHideNativeContextMenu(
  event: ContextMenuEventLike,
  selection: ContextMenuSelection | null,
  { allowInspect = false }: { allowInspect?: boolean } = {},
): boolean {
  // A custom menu already claimed the click.
  if (event.defaultPrevented) return false;
  if (allowInspect && event.shiftKey) return false;
  const element = targetElement(event.target);
  if (element && hasNativeMenu(element)) return false;
  return !isOnSelection(selection, event.clientX, event.clientY);
}

/**
 * Listens in the bubble phase on the window, after React's handlers on the
 * root, so custom menus see an untouched event and their `preventDefault`
 * is respected. Returns a function that removes the listener.
 */
export function installNativeContextMenuGuard(
  target: Window = window,
  allowInspect: boolean = import.meta.env.DEV,
): () => void {
  const onContextMenu = (event: MouseEvent) => {
    if (
      shouldHideNativeContextMenu(event, target.getSelection(), {
        allowInspect,
      })
    ) {
      event.preventDefault();
    }
  };
  target.addEventListener("contextmenu", onContextMenu);
  return () => target.removeEventListener("contextmenu", onContextMenu);
}
