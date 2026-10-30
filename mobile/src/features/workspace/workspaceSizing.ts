/** Leave at least 600 points for the conversation beside a 320-point sidebar.
 * Use window width so iPad windowing and Split View collapse naturally. */
export const SIDEBAR_WIDTH = 320;
export const WIDE_WORKSPACE = 920;

export function workspaceLayout(width: number, collapsed: boolean) {
  const wide = width >= WIDE_WORKSPACE;
  const sidebarWidth = wide && !collapsed ? SIDEBAR_WIDTH : 0;
  return { wide, sidebarWidth, paneWidth: width - sidebarWidth };
}
