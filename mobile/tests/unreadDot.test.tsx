import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const app = vi.hoisted(() => ({ unreadMonos: new Set<string>() }));
vi.mock("react-native", () => ({ View: () => null }));
vi.mock("@/ui/theme", () => ({ themedStyles: () => () => ({}) }));
vi.mock("@/features/connection/ConnectionProvider", () => ({
  useConnection: () => ({ pairing: { desktop: { fingerprint: "desk" } } }),
}));
vi.mock("@/features/workspace/WorkspaceProvider", () => ({ useWorkspace: () => ({ monos: [] }) }));
vi.mock("@/features/workspace/monoSeen", () => ({ useUnreadMonos: () => app.unreadMonos }));

import { resetSessionActivity, trackSessions } from "@/features/sessions/activityStore";
import { unreadTotal, useAnyUnread } from "@/features/workspace/UnreadDot";
import type { SessionSummary } from "@/features/workspace/types";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const session = (id: string, busy: boolean, projectId = "p1") =>
  ({ id, projectId, title: id, busy, updatedAt: 1 }) as SessionSummary;

describe("the dot on the drawer's button", () => {
  let root: ReturnType<typeof createRoot>;
  let dotted: boolean;
  function Probe() {
    dotted = useAnyUnread();
    return null;
  }
  beforeEach(() => {
    app.unreadMonos = new Set();
    resetSessionActivity();
    root = createRoot(document.createElement("div"));
    act(() => root.render(<Probe />));
  });
  afterEach(() => act(() => root.unmount()));

  it("adds unread Monos to finished sessions", () => {
    expect(unreadTotal(0, 0)).toBe(0);
    expect(unreadTotal(2, 3)).toBe(5);
  });

  it("is off with nothing to see", () => {
    expect(dotted).toBe(false);
  });

  it("is on for an unread Mono", () => {
    app.unreadMonos = new Set(["m"]);
    act(() => root.render(<Probe />));
    expect(dotted).toBe(true);
  });

  it("is on for a session that finished, in any project, until it is opened", () => {
    act(() => trackSessions([session("a", true, "p2")]));
    expect(dotted).toBe(false);
    act(() => trackSessions([session("a", false, "p2")]));
    expect(dotted).toBe(true);
    act(() => resetSessionActivity());
    expect(dotted).toBe(false);
  });
});
