import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { useTerminalActions } from "@/features/terminal/terminalActions";

const desktop = vi.hoisted(() => ({
  request: vi.fn(),
  refresh: vi.fn(),
  pairing: {} as object | null,
  actions: [{ id: "a", name: "Run Dev", command: "npm run dev", icon: "play" }],
  others: [{ id: "o", name: "Elsewhere", command: "make", icon: "hammer" }],
}));
vi.mock("@/features/connection/nativeLink", () => ({
  request: desktop.request,
}));
vi.mock("@/features/connection/ConnectionProvider", () => ({
  useConnection: () => ({ pairing: desktop.pairing, refresh: desktop.refresh }),
}));
vi.mock("@/features/workspace/WorkspaceProvider", () => ({
  useWorkspace: () => ({
    projects: [
      { id: "other", actions: desktop.others },
      { id: "p", actions: desktop.actions },
      { id: "bare" },
    ],
  }),
}));

let seen: ReturnType<typeof useTerminalActions>;
const see = (value: typeof seen) => {
  seen = value;
};
function Probe({ project = "p" }: { project?: string }) {
  see(useTerminalActions(project));
  return null;
}

let root: ReturnType<typeof createRoot>;
const initial = desktop.actions;
beforeEach(() => {
  (
    globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }
  ).IS_REACT_ACT_ENVIRONMENT = true;
  desktop.pairing = {};
  desktop.actions = initial;
  desktop.request.mockReset().mockResolvedValue({});
  desktop.refresh.mockReset().mockResolvedValue(undefined);
  root = createRoot(document.createElement("div"));
  act(() => root.render(<Probe />));
});
afterEach(() => act(() => root.unmount()));

it("shows the desktop's actions and sends the whole list when one is removed", async () => {
  expect(seen[0]).toEqual(desktop.actions);
  await act(() => seen[1]([]));
  expect(desktop.request).toHaveBeenCalledWith({}, "terminal.actions.save", {
    projectId: "p",
    actions: [],
  });
  expect(desktop.refresh).toHaveBeenCalled();
});

it("shows a change at once, while it is on its way", async () => {
  let finish!: () => void;
  desktop.request.mockReturnValue(
    new Promise<void>((resolve) => (finish = resolve)),
  );
  const added = [
    ...desktop.actions,
    { name: "Test", command: "cargo test", icon: "play" },
  ];
  let saving!: Promise<void>;
  act(() => {
    saving = seen[1](added);
  });
  expect(seen[0]).toEqual(added);
  await act(async () => {
    finish();
    await saving;
  });
  expect(seen[0]).toEqual(desktop.actions);
});

it("shows an edit made on the desktop once the workspace brings it", () => {
  desktop.actions = [
    ...desktop.actions,
    { id: "b", name: "Deploy", command: "fly deploy", icon: "rocket" },
  ];
  act(() => root.render(<Probe />));
  expect(seen[0].map((action) => action.name)).toEqual(["Run Dev", "Deploy"]);
});

it("keeps each project's actions to itself, and none for a project without", () => {
  act(() => root.render(<Probe project="other" />));
  expect(seen[0]).toEqual(desktop.others);
  act(() => root.render(<Probe project="bare" />));
  expect(seen[0]).toEqual([]);
});

it("goes back to the desktop's list when it refuses a change, and says why", async () => {
  desktop.request.mockRejectedValue(new Error("Too many actions"));
  await act(async () => {
    await expect(seen[1]([])).rejects.toThrow("Too many actions");
  });
  expect(seen[0]).toEqual(desktop.actions);
  expect(desktop.refresh).not.toHaveBeenCalled();
});

it("sends nothing without a desktop to send it to", async () => {
  desktop.pairing = null;
  act(() => root.render(<Probe />));
  await act(() => seen[1]([]));
  expect(desktop.request).not.toHaveBeenCalled();
  expect(seen[0]).toEqual(desktop.actions);
});
