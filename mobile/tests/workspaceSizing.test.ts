import { expect, it } from "vitest";
import { workspaceLayout } from "@/features/workspace/workspaceSizing";

it("only shows a sidebar when both panes fit, including resized iPad windows", () => {
  expect(workspaceLayout(919, false)).toEqual({
    wide: false,
    sidebarWidth: 0,
    paneWidth: 919,
  });
  expect(workspaceLayout(920, false)).toEqual({
    wide: true,
    sidebarWidth: 320,
    paneWidth: 600,
  });
  expect(workspaceLayout(1180, false).paneWidth).toBe(860);
  expect(workspaceLayout(1180, true)).toEqual({
    wide: true,
    sidebarWidth: 0,
    paneWidth: 1180,
  });
});
