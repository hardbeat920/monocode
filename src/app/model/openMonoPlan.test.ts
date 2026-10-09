import { expect, it, vi } from "vitest";
import { newSession } from "../../features/sessions/model/session";
import { newTab } from "../../features/workspace/model/layout";
import { openMonoPlan } from "./openMonoPlan";

it("opens a valid plan and reveals the workspace", () => {
  const session = {
    ...newSession("codex", "/repo"),
    blocks: [{ id: "plan", role: "plan" as const, text: "# Plan" }],
  };
  const tab = newTab(session.id);
  let currentTabs = [tab];
  const closeMonoView = vi.fn();
  const setTabs = vi.fn(
    (update: (tabs: typeof currentTabs) => typeof currentTabs) => {
      currentTabs = update(currentTabs);
    },
  );
  const setComposerFocused = vi.fn();

  expect(
    openMonoPlan({
      tab,
      session,
      blockId: "plan",
      closeMonoView,
      setTabs,
      setComposerFocused,
    }),
  ).toBe(true);
  expect(closeMonoView).toHaveBeenCalledOnce();
  expect(setComposerFocused).toHaveBeenCalledWith(false);
  expect(
    currentTabs[0].editorPanes.flatMap((pane) => pane.files),
  ).toContainEqual(expect.objectContaining({ path: "plan:plan" }));
});

it("leaves the floating Mono open when the plan is stale", () => {
  const session = newSession("codex", "/repo");
  const closeMonoView = vi.fn();
  const setTabs = vi.fn();

  expect(
    openMonoPlan({
      tab: newTab(session.id),
      session,
      blockId: "stale-plan",
      closeMonoView,
      setTabs,
      setComposerFocused: vi.fn(),
    }),
  ).toBe(false);
  expect(closeMonoView).not.toHaveBeenCalled();
  expect(setTabs).not.toHaveBeenCalled();
});
