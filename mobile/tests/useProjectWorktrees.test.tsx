import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { useProjectWorktrees } from "@/features/connection/useProjectWorktrees";

const desktop = vi.hoisted(() => ({
  command: vi.fn(),
  connected: true,
  pairing: { desktop: { fingerprint: "first" } },
}));
vi.mock("@/features/connection/ConnectionProvider", () => ({ useConnection: () => desktop }));

it("requires a live per-project capability and drops it when the desktop changes, disconnects, or omits support", async () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const container = document.createElement("div");
  const root = createRoot(container);
  let supported = false;
  function Probe({ project }: { project: string }) {
    supported = useProjectWorktrees(project).cowSupported;
    return null;
  }
  const render = async (project = "project") => act(async () => {
    root.render(<Probe project={project} />);
    await Promise.resolve();
  });
  let resolve!: (value: object) => void;
  desktop.command.mockImplementation(() => new Promise(r => { resolve = r; }));
  try {
    await render();
    expect(supported).toBe(false);
    await act(async () => resolve({ worktrees: [], cowSupported: true }));
    expect(supported).toBe(true);
    desktop.connected = false;
    await render();
    expect(supported).toBe(false);
    desktop.connected = true;
    await render();
    expect(supported).toBe(false);
    await act(async () => resolve({ worktrees: [], cowSupported: true }));
    expect(supported).toBe(true);
    desktop.pairing = { desktop: { fingerprint: "older-desktop" } };
    await render();
    expect(supported).toBe(false);
    await act(async () => resolve({ worktrees: [] }));
    expect(supported).toBe(false);
    await render("unsupported-project");
    await act(async () => resolve({ worktrees: [], cowSupported: false }));
    expect(supported).toBe(false);
  } finally {
    act(() => root.unmount());
  }
});
