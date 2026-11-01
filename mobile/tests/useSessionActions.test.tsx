import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useSessionActions } from "@/features/sessions/useSessionActions";
import type { SessionSummary } from "@/features/workspace/types";

const app = vi.hoisted(() => ({
  alert: vi.fn(),
  archive: vi.fn(async (_id: string) => {}),
  unarchive: vi.fn(async (_id: string) => {}),
  canUnarchive: true,
}));
vi.mock("react-native", () => ({
  Alert: { alert: app.alert, prompt: vi.fn() },
  ActionSheetIOS: { showActionSheetWithOptions: vi.fn() },
}));
vi.mock("expo-haptics", () => ({
  impactAsync: vi.fn(),
  notificationAsync: vi.fn(),
  ImpactFeedbackStyle: { Medium: "medium" },
  NotificationFeedbackType: { Success: "success" },
}));
vi.mock("@/ui/theme", () => ({ useScheme: () => "light" }));
vi.mock("@/features/workspace/WorkspaceProvider", () => ({
  useWorkspace: () => ({
    renameSession: vi.fn(),
    deleteSession: vi.fn(async () => {}),
    archiveSession: app.archive,
    unarchiveSession: app.unarchive,
    canUnarchive: app.canUnarchive,
  }),
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const session = { id: "s1", title: "Fix the build" } as SessionSummary;

describe("archiving from a session row", () => {
  let root: ReturnType<typeof createRoot>;
  let actions: ReturnType<typeof useSessionActions>;
  const toast = { show: vi.fn(), dismiss: vi.fn() };
  function Probe({ withToast }: { withToast: boolean }) {
    actions = useSessionActions(() => {}, withToast ? toast : undefined);
    return null;
  }
  const render = (withToast = true) => act(() => root.render(<Probe withToast={withToast} />));
  const settle = () => act(async () => { await new Promise((done) => setTimeout(done, 0)); });

  beforeEach(() => {
    vi.clearAllMocks();
    app.canUnarchive = true;
    root = createRoot(document.createElement("div"));
  });
  afterEach(() => act(() => root.unmount()));

  it("archives at once, with no question, when the desktop can take it back", async () => {
    render();
    act(() => actions.archive(session));
    await settle();
    expect(app.alert).not.toHaveBeenCalled();
    expect(app.archive).toHaveBeenCalledWith("s1");
    expect(toast.show).toHaveBeenCalledWith("Archived “Fix the build”", expect.any(Function));
  });

  it("asks first, and offers no Undo, when it can't be taken back", async () => {
    app.canUnarchive = false;
    render();
    act(() => actions.archive(session));
    expect(app.archive).not.toHaveBeenCalled();
    expect(app.alert.mock.calls[0][0]).toBe("Archive “Fix the build”?");
    await act(async () => app.alert.mock.calls[0][2][1].onPress());
    await settle();
    expect(app.archive).toHaveBeenCalledWith("s1");
    expect(toast.show).toHaveBeenCalledWith("Archived “Fix the build”", undefined);
  });

  it("marks the row as removing while it goes", async () => {
    render();
    act(() => actions.archive(session));
    expect(actions.removing?.id).toBe("s1");
    await act(async () => { await new Promise((done) => setTimeout(done, 300)); });
    expect(actions.removing).toBeNull();
  });

  it("says the session is back when the desktop refuses", async () => {
    app.archive.mockRejectedValueOnce(new Error("Desktop is busy"));
    render();
    act(() => actions.archive(session));
    await settle();
    expect(toast.show).toHaveBeenLastCalledWith(expect.stringContaining("It’s back in the list."));
  });

  it("is not offered where no toast is there to say what it did", () => {
    render(false);
    act(() => actions.archive(session));
    expect(app.archive).not.toHaveBeenCalled();
    expect(app.alert).not.toHaveBeenCalled();
  });

  it("asks before archiving a Mono's finished sessions, then archives each", async () => {
    render();
    act(() => actions.archiveFinished([session, { ...session, id: "s2" }], "TaskGiver"));
    expect(app.archive).not.toHaveBeenCalled();
    expect(app.alert.mock.calls[0][0]).toBe("Archive 2 finished sessions from TaskGiver?");
    await act(async () => app.alert.mock.calls[0][2][1].onPress());
    await settle();
    expect(app.archive.mock.calls.map(([id]) => id)).toEqual(["s1", "s2"]);
    expect(toast.show).toHaveBeenLastCalledWith("Archived 2 sessions");
    // Nothing to ask about when nothing is finished.
    app.alert.mockClear();
    act(() => actions.archiveFinished([], "TaskGiver"));
    expect(app.alert).not.toHaveBeenCalled();
  });
});
