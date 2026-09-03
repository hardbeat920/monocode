/**
 * Dev-only Click-to-Component Inspector for MonoCode
 * - Press & hold Option (⌥) to inspect React components.
 * - Left click to open the component directly in the configured editor (Cursor, VS Code, Zed, etc.).
 * - Right click to open the Component Hierarchy Tree panel with editor switcher.
 */

export type SupportedEditor =
  | "cursor"
  | "code"
  | "code-insiders"
  | "windsurf"
  | "zed"
  | "webstorm"
  | "subl";

export const EDITOR_OPTIONS: Array<{ id: SupportedEditor; label: string }> = [
  { id: "cursor", label: "Cursor" },
  { id: "code", label: "VS Code" },
  { id: "code-insiders", label: "VS Code Insiders" },
  { id: "windsurf", label: "Windsurf" },
  { id: "zed", label: "Zed" },
  { id: "webstorm", label: "WebStorm" },
  { id: "subl", label: "Sublime" },
];

export function getPreferredEditor(): SupportedEditor {
  if (typeof window !== "undefined") {
    const saved = localStorage.getItem("monocode_inspector_editor") as SupportedEditor | null;
    if (saved && EDITOR_OPTIONS.some((opt) => opt.id === saved)) {
      return saved;
    }
  }
  const envEditor = (import.meta.env.VITE_INSPECTOR_EDITOR as string | undefined)?.toLowerCase();
  if (envEditor && EDITOR_OPTIONS.some((opt) => opt.id === envEditor)) {
    return envEditor as SupportedEditor;
  }
  return "cursor";
}

export function setPreferredEditor(editor: SupportedEditor): void {
  if (typeof window !== "undefined") {
    localStorage.setItem("monocode_inspector_editor", editor);
  }
}

export function getEditorLabel(editor: SupportedEditor): string {
  return EDITOR_OPTIONS.find((opt) => opt.id === editor)?.label || "Editor";
}

interface ComponentNode {
  comp: string;
  file: string;
  line: string;
  col: string;
  element: HTMLElement;
}

let active = false;
let panelOpen = false;
let overlayEl: HTMLDivElement | null = null;
let badgeEl: HTMLDivElement | null = null;
let hierarchyPanelEl: HTMLDivElement | null = null;

const PADDING = 16;

function normalizePath(fullPath: string): string {
  const marker = "/src/";
  const idx = fullPath.lastIndexOf(marker);
  return idx !== -1 ? fullPath.slice(idx + 1) : fullPath;
}

function openInEditor(file: string, line: string, col: string): void {
  const editor = getPreferredEditor();
  void fetch(
    `/__open_in_editor?file=${encodeURIComponent(file)}&line=${line}&col=${col}&editor=${encodeURIComponent(editor)}`
  );
}

function createOverlay(): void {
  if (overlayEl) return;

  const styleTag = document.createElement("style");
  styleTag.textContent = `
    @keyframes monocodePanelFadeIn {
      from { opacity: 0; transform: scale(0.97) translateY(-4px); }
      to { opacity: 1; transform: scale(1) translateY(0); }
    }
    #monocode-hierarchy-panel *::-webkit-scrollbar {
      width: 6px;
    }
    #monocode-hierarchy-panel *::-webkit-scrollbar-thumb {
      background: rgba(255, 255, 255, 0.15);
      border-radius: 3px;
    }
  `;
  document.head.appendChild(styleTag);

  overlayEl = document.createElement("div");
  overlayEl.id = "monocode-component-inspector-overlay";
  overlayEl.style.cssText = `
    position: fixed;
    pointer-events: none;
    z-index: 2147483640;
    border: 2px solid #38bdf8;
    background: rgba(56, 189, 248, 0.08);
    border-radius: 4px;
    box-shadow: 0 0 16px rgba(56, 189, 248, 0.35);
    transition: all 0.06s cubic-bezier(0.16, 1, 0.3, 1);
    display: none;
  `;

  badgeEl = document.createElement("div");
  badgeEl.id = "monocode-component-inspector-badge";
  badgeEl.style.cssText = `
    position: fixed;
    pointer-events: none;
    z-index: 2147483641;
    background: rgba(18, 18, 22, 0.94);
    backdrop-filter: blur(16px) saturate(180%);
    -webkit-backdrop-filter: blur(16px) saturate(180%);
    border: 1px solid rgba(56, 189, 248, 0.35);
    border-radius: 8px;
    padding: 6px 10px;
    box-shadow: 0 12px 32px rgba(0, 0, 0, 0.65), 0 0 0 1px rgba(255, 255, 255, 0.06);
    color: #f4f4f5;
    font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
    display: none;
    max-width: 480px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  `;

  document.documentElement.appendChild(overlayEl);
  document.documentElement.appendChild(badgeEl);
}

function highlightElement(target: HTMLElement | null): void {
  if (!overlayEl) return;

  if (!target) {
    overlayEl.style.display = "none";
    return;
  }

  const rect = target.getBoundingClientRect();
  if (rect.width === 0 && rect.height === 0) return;

  overlayEl.style.display = "block";
  overlayEl.style.left = `${rect.left}px`;
  overlayEl.style.top = `${rect.top}px`;
  overlayEl.style.width = `${rect.width}px`;
  overlayEl.style.height = `${rect.height}px`;
}

function updateHighlight(
  target: HTMLElement | null,
  mouseX?: number,
  mouseY?: number
): void {
  if (!overlayEl || !badgeEl) return;

  if (!target || !active) {
    overlayEl.style.display = "none";
    badgeEl.style.display = "none";
    return;
  }

  highlightElement(target);

  const rect = target.getBoundingClientRect();
  const comp = target.getAttribute("data-insp-comp") || target.tagName.toLowerCase();
  const file = target.getAttribute("data-insp-file") || "";
  const line = target.getAttribute("data-insp-line") || "1";
  const displayPath = normalizePath(file);
  const editorLabel = getEditorLabel(getPreferredEditor());

  badgeEl.innerHTML = `
    <div style="display: flex; align-items: center; gap: 8px;">
      <span style="color: #38bdf8; font-weight: 700; font-size: 12px;">&lt;${comp}&gt;</span>
      <span style="color: #94a3b8; font-size: 11px;">${displayPath}:${line}</span>
    </div>
    <div style="color: #64748b; font-size: 10px; margin-top: 2px;">⌥ Click to open in ${editorLabel} • ⌥ Right-click for tree</div>
  `;

  badgeEl.style.display = "block";
  const badgeWidth = badgeEl.offsetWidth || 280;
  const badgeHeight = badgeEl.offsetHeight || 44;
  const BADGE_PADDING = 12;

  let badgeLeft: number;
  let badgeTop: number;

  if (mouseX !== undefined && mouseY !== undefined) {
    // Follow mouse cursor with collision detection
    badgeLeft = mouseX + 14;
    if (badgeLeft + badgeWidth > window.innerWidth - BADGE_PADDING) {
      badgeLeft = mouseX - badgeWidth - 14;
    }

    badgeTop = mouseY + 16;
    if (badgeTop + badgeHeight > window.innerHeight - BADGE_PADDING) {
      badgeTop = mouseY - badgeHeight - 14;
    }
  } else {
    // Element-anchored fallback
    badgeLeft = rect.left;
    if (badgeLeft + badgeWidth > window.innerWidth - BADGE_PADDING) {
      badgeLeft = window.innerWidth - badgeWidth - BADGE_PADDING;
    }

    badgeTop = rect.top - badgeHeight - 6;
    if (badgeTop < BADGE_PADDING) {
      badgeTop = rect.bottom + 6;
      if (badgeTop + badgeHeight > window.innerHeight - BADGE_PADDING) {
        badgeTop = window.innerHeight - badgeHeight - BADGE_PADDING;
      }
    }
  }

  // Strict clamp within viewport
  badgeLeft = Math.max(BADGE_PADDING, Math.min(badgeLeft, window.innerWidth - badgeWidth - BADGE_PADDING));
  badgeTop = Math.max(BADGE_PADDING, Math.min(badgeTop, window.innerHeight - badgeHeight - BADGE_PADDING));

  badgeEl.style.left = `${badgeLeft}px`;
  badgeEl.style.top = `${badgeTop}px`;
}

function closeHierarchyPanel(): void {
  if (hierarchyPanelEl) {
    hierarchyPanelEl.remove();
    hierarchyPanelEl = null;
  }
  panelOpen = false;
  active = false;
  highlightElement(null);
  if (badgeEl) badgeEl.style.display = "none";
  document.body.style.cursor = "";
}

function showHierarchyPanel(target: HTMLElement, x: number, y: number): void {
  closeHierarchyPanel();

  // Reset body cursor to default while user interacts with the panel
  document.body.style.cursor = "";

  // Hide hover badge
  if (badgeEl) badgeEl.style.display = "none";

  // Build component hierarchy from target (leaf) up to root (App)
  let curr: HTMLElement | null = target;
  const hierarchy: ComponentNode[] = [];
  const seenComps = new Set<string>();

  while (curr && curr !== document.body && curr !== document.documentElement) {
    const file = curr.getAttribute("data-insp-file");
    const comp = curr.getAttribute("data-insp-comp");
    const line = curr.getAttribute("data-insp-line") || "1";
    const col = curr.getAttribute("data-insp-col") || "1";

    if (file && comp && /^[A-Z]/.test(comp)) {
      if (!seenComps.has(comp)) {
        seenComps.add(comp);
        hierarchy.push({ comp, file, line, col, element: curr });
      }
    }
    curr = curr.parentElement;
  }

  if (hierarchy.length === 0) {
    const file = target.getAttribute("data-insp-file");
    if (file) {
      const comp = target.getAttribute("data-insp-comp") || target.tagName.toLowerCase();
      const line = target.getAttribute("data-insp-line") || "1";
      const col = target.getAttribute("data-insp-col") || "1";
      hierarchy.push({ comp, file, line, col, element: target });
    }
  }

  if (hierarchy.length === 0) return;

  panelOpen = true;

  const panel = document.createElement("div");
  hierarchyPanelEl = panel;
  panel.id = "monocode-hierarchy-panel";
  panel.style.cssText = `
    position: fixed;
    z-index: 2147483647;
    width: 480px;
    max-width: calc(100vw - 32px);
    max-height: calc(100vh - 32px);
    display: flex;
    flex-direction: column;
    background: rgba(18, 18, 22, 0.96);
    backdrop-filter: blur(24px) saturate(190%);
    -webkit-backdrop-filter: blur(24px) saturate(190%);
    border: 1px solid rgba(255, 255, 255, 0.12);
    border-radius: 12px;
    box-shadow: 0 24px 64px rgba(0, 0, 0, 0.8), 0 0 0 1px rgba(56, 189, 248, 0.25);
    color: #f4f4f5;
    font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
    overflow: hidden;
    user-select: none;
    animation: monocodePanelFadeIn 0.15s cubic-bezier(0.16, 1, 0.3, 1);
  `;

  const currentEditor = getPreferredEditor();

  // Header (flex-shrink: 0)
  const header = document.createElement("div");
  header.style.cssText = `
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 14px;
    border-bottom: 1px solid rgba(255, 255, 255, 0.08);
    background: rgba(255, 255, 255, 0.03);
    cursor: grab;
    flex-shrink: 0;
  `;
  header.innerHTML = `
    <div style="display: flex; align-items: center; gap: 8px; font-weight: 600; font-size: 13px;">
      <span style="color: #38bdf8; font-size: 14px;">◈</span>
      <span>Component Hierarchy</span>
      <span style="color: #71717a; font-size: 11px; font-weight: normal;">(${hierarchy.length} levels)</span>
    </div>
    <div style="display: flex; align-items: center; gap: 8px;">
      <select id="monocode-hierarchy-editor-select" title="Open in editor" style="
        background: rgba(255, 255, 255, 0.08);
        color: #38bdf8;
        border: 1px solid rgba(56, 189, 248, 0.3);
        border-radius: 6px;
        padding: 2px 6px;
        font-size: 11px;
        font-family: inherit;
        font-weight: 600;
        cursor: pointer;
        outline: none;
      ">
        ${EDITOR_OPTIONS.map(
          (opt) =>
            `<option value="${opt.id}" ${opt.id === currentEditor ? "selected" : ""} style="background: #18181b; color: #f4f4f5;">${opt.label}</option>`
        ).join("")}
      </select>
      <button id="monocode-hierarchy-close" style="
        background: none;
        border: none;
        color: #a1a1aa;
        cursor: pointer;
        font-size: 14px;
        padding: 2px 6px;
        border-radius: 4px;
        line-height: 1;
        transition: color 0.15s;
      ">✕</button>
    </div>
  `;

  // Close button
  const closeBtn = header.querySelector("#monocode-hierarchy-close");
  closeBtn?.addEventListener("click", () => {
    closeHierarchyPanel();
  });

  // Editor select handler
  const editorSelect = header.querySelector("#monocode-hierarchy-editor-select") as HTMLSelectElement | null;
  editorSelect?.addEventListener("change", (e) => {
    const val = (e.target as HTMLSelectElement).value as SupportedEditor;
    setPreferredEditor(val);
    const label = getEditorLabel(val);
    const footerHint = panel.querySelector("#monocode-hierarchy-footer-hint");
    if (footerHint) {
      footerHint.textContent = `Click any node to open in ${label}`;
    }
  });

  // Tree List (flex: 1 1 auto; min-height: 0; overflow-y: auto;)
  const list = document.createElement("div");
  list.style.cssText = `
    flex: 1 1 auto;
    min-height: 0;
    overflow-y: auto;
    padding: 8px;
  `;

  hierarchy.forEach((item, idx) => {
    const isTarget = idx === 0;
    const isRoot = idx === hierarchy.length - 1;

    let branch = "├─ ";
    if (isTarget) branch = "● ";
    else if (isRoot) branch = "└─ ";

    const row = document.createElement("div");
    row.style.cssText = `
      display: flex;
      align-items: center;
      gap: 6px;
      padding: 6px 10px;
      padding-left: ${8 + idx * 14}px;
      border-radius: 6px;
      cursor: pointer;
      font-size: 12px;
      transition: background 0.15s ease, color 0.15s ease;
      position: relative;
    `;

    const displayPath = normalizePath(item.file);

    row.innerHTML = `
      <span style="color: #52525b; user-select: none;">${branch}</span>
      <span style="color: ${isTarget ? '#38bdf8' : '#e4e4e7'}; font-weight: 700;">&lt;${item.comp}&gt;</span>
      ${
        isTarget
          ? `<span style="background: rgba(56, 189, 248, 0.18); color: #38bdf8; font-size: 10px; padding: 1px 5px; border-radius: 4px; font-weight: 600;">target</span>`
          : ""
      }
      <span style="color: #71717a; font-size: 11px; margin-left: auto; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; max-width: 200px;">
        ${displayPath}:${item.line}
      </span>
    `;

    row.addEventListener("mouseenter", () => {
      row.style.background = "rgba(56, 189, 248, 0.14)";
      highlightElement(item.element);
    });

    row.addEventListener("mouseleave", () => {
      row.style.background = "transparent";
      highlightElement(target);
    });

    row.addEventListener("click", (e) => {
      e.stopPropagation();
      const currentLabel = getEditorLabel(getPreferredEditor());
      row.style.background = "rgba(34, 197, 94, 0.25)";
      row.innerHTML = `
        <span style="color: #22c55e; font-weight: 700; padding: 2px 0;">✓ Opening &lt;${item.comp}&gt; in ${currentLabel}...</span>
      `;
      openInEditor(item.file, item.line, item.col);
      setTimeout(() => {
        closeHierarchyPanel();
      }, 350);
    });

    list.appendChild(row);
  });

  // Footer (flex-shrink: 0)
  const footer = document.createElement("div");
  footer.style.cssText = `
    padding: 8px 14px;
    border-top: 1px solid rgba(255, 255, 255, 0.06);
    background: rgba(0, 0, 0, 0.25);
    display: flex;
    justify-content: space-between;
    align-items: center;
    font-size: 11px;
    color: #71717a;
    flex-shrink: 0;
  `;
  footer.innerHTML = `
    <span id="monocode-hierarchy-footer-hint">Click any node to open in ${getEditorLabel(currentEditor)}</span>
    <span style="font-size: 10px; color: #52525b;">Esc to close</span>
  `;

  // Assemble child elements completely INTO panel BEFORE measuring dimensions!
  panel.appendChild(header);
  panel.appendChild(list);
  panel.appendChild(footer);

  // Attach to documentElement with visibility: hidden; position: fixed; left: 0; top: 0; to measure real bounding rect via getBoundingClientRect()
  panel.style.visibility = "hidden";
  panel.style.position = "fixed";
  panel.style.left = "0px";
  panel.style.top = "0px";
  document.documentElement.appendChild(panel);

  const rect = panel.getBoundingClientRect();
  const panelWidth = rect.width;
  const panelHeight = rect.height;

  // Horizontal collision: default x + 12. If left + panelWidth > window.innerWidth - PADDING, flip to x - panelWidth - 12. Clamp left between PADDING and window.innerWidth - panelWidth - PADDING.
  let left = x + 12;
  if (left + panelWidth > window.innerWidth - PADDING) {
    left = x - panelWidth - 12;
  }
  left = Math.max(PADDING, Math.min(left, window.innerWidth - panelWidth - PADDING));

  // Vertical collision: default y + 12. If top + panelHeight > window.innerHeight - PADDING, flip to y - panelHeight - 12. Clamp top between PADDING and window.innerHeight - panelHeight - PADDING.
  let top = y + 12;
  if (top + panelHeight > window.innerHeight - PADDING) {
    top = y - panelHeight - 12;
  }
  top = Math.max(PADDING, Math.min(top, window.innerHeight - panelHeight - PADDING));

  // Apply calculated left and top, then set visibility: visible
  panel.style.left = `${left}px`;
  panel.style.top = `${top}px`;
  panel.style.visibility = "visible";

  // Dragging support with collision boundaries and re-clamping
  header.addEventListener("mousedown", (e) => {
    const targetEl = e.target as HTMLElement;
    if (
      targetEl.id === "monocode-hierarchy-close" ||
      targetEl.id === "monocode-hierarchy-editor-select" ||
      targetEl.closest("#monocode-hierarchy-editor-select")
    ) {
      return;
    }

    let isDragging = true;
    const startX = e.clientX;
    const startY = e.clientY;
    const origLeft = panel.offsetLeft;
    const origTop = panel.offsetTop;
    header.style.cursor = "grabbing";

    const onMouseMove = (moveEv: MouseEvent) => {
      if (!isDragging) return;
      let newLeft = origLeft + (moveEv.clientX - startX);
      let newTop = origTop + (moveEv.clientY - startY);

      const pRect = panel.getBoundingClientRect();
      const pWidth = pRect.width;
      const pHeight = pRect.height;

      newLeft = Math.max(PADDING, Math.min(newLeft, window.innerWidth - pWidth - PADDING));
      newTop = Math.max(PADDING, Math.min(newTop, window.innerHeight - pHeight - PADDING));

      panel.style.left = `${newLeft}px`;
      panel.style.top = `${newTop}px`;
    };

    const onMouseUp = () => {
      if (isDragging) {
        isDragging = false;
        header.style.cursor = "grab";
      }
      window.removeEventListener("mousemove", onMouseMove);
      window.removeEventListener("mouseup", onMouseUp);
    };

    window.addEventListener("mousemove", onMouseMove);
    window.addEventListener("mouseup", onMouseUp);
  });

  highlightElement(target);
}

function deactivate(): void {
  active = false;
  if (!panelOpen) {
    updateHighlight(null);
    document.body.style.cursor = "";
  }
}

function activate(): void {
  if (active) return;
  active = true;
  createOverlay();
  document.body.style.cursor = "crosshair";
}

export function initComponentInspector(): void {
  if (typeof window === "undefined") return;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  if ((window as any).__MONOCODE_INSPECTOR_INITIALIZED__) return;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  (window as any).__MONOCODE_INSPECTOR_INITIALIZED__ = true;

  createOverlay();

  window.addEventListener(
    "keydown",
    (e) => {
      if (e.key === "Alt" || e.altKey) {
        activate();
      }
      if (e.key === "Escape") {
        if (panelOpen) {
          closeHierarchyPanel();
        } else {
          deactivate();
        }
      }
    },
    true
  );

  window.addEventListener(
    "keyup",
    (e) => {
      if (!e.altKey && active) {
        deactivate();
      }
    },
    true
  );

  window.addEventListener("blur", () => {
    deactivate();
  });

  window.addEventListener(
    "mousemove",
    (e) => {
      if (!active || panelOpen) return;
      const target = (e.target as HTMLElement | null)?.closest(
        "[data-insp-file]"
      ) as HTMLElement | null;
      updateHighlight(target, e.clientX, e.clientY);
    },
    true
  );

  window.addEventListener("resize", () => {
    if (panelOpen && hierarchyPanelEl) {
      const pRect = hierarchyPanelEl.getBoundingClientRect();
      const pWidth = pRect.width;
      const pHeight = pRect.height;
      const curLeft = hierarchyPanelEl.offsetLeft;
      const curTop = hierarchyPanelEl.offsetTop;
      const clampedLeft = Math.max(PADDING, Math.min(curLeft, window.innerWidth - pWidth - PADDING));
      const clampedTop = Math.max(PADDING, Math.min(curTop, window.innerHeight - pHeight - PADDING));
      hierarchyPanelEl.style.left = `${clampedLeft}px`;
      hierarchyPanelEl.style.top = `${clampedTop}px`;
    }
  });

  // Left click: Open directly in configured editor
  window.addEventListener(
    "click",
    (e) => {
      // If panel is open and click was outside, close panel
      if (panelOpen && hierarchyPanelEl) {
        if (!hierarchyPanelEl.contains(e.target as Node)) {
          closeHierarchyPanel();
        }
        return;
      }

      if (!active && !e.altKey) return;

      const target = (e.target as HTMLElement | null)?.closest(
        "[data-insp-file]"
      ) as HTMLElement | null;

      if (!target) return;

      e.preventDefault();
      e.stopPropagation();
      e.stopImmediatePropagation();

      const file = target.getAttribute("data-insp-file");
      const line = target.getAttribute("data-insp-line") || "1";
      const col = target.getAttribute("data-insp-col") || "1";
      const editorLabel = getEditorLabel(getPreferredEditor());

      if (file && badgeEl) {
        badgeEl.innerHTML = `
          <div style="display: flex; align-items: center; gap: 8px; color: #22c55e;">
            <span style="font-weight: 700; font-size: 12px;">✓ Opening in ${editorLabel}...</span>
          </div>
        `;
        badgeEl.style.borderColor = "rgba(34, 197, 94, 0.6)";

        openInEditor(file, line, col);

        setTimeout(() => {
          if (badgeEl) {
            badgeEl.style.borderColor = "rgba(56, 189, 248, 0.35)";
          }
          deactivate();
        }, 400);
      }
    },
    true
  );

  // Right click: Open Component Hierarchy Tree
  window.addEventListener(
    "contextmenu",
    (e) => {
      if (!active && !e.altKey) return;

      const target = (e.target as HTMLElement | null)?.closest(
        "[data-insp-file]"
      ) as HTMLElement | null;

      if (!target) return;

      e.preventDefault();
      e.stopPropagation();
      e.stopImmediatePropagation();

      showHierarchyPanel(target, e.clientX, e.clientY);
    },
    true
  );
}

// Auto-initialize when window is defined with guard
if (typeof window !== "undefined") {
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  if (!(window as any).__MONOCODE_INSPECTOR_INITIALIZED__) {
    initComponentInspector();
  }
}
