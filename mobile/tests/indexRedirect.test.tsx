import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const app = vi.hoisted(() => ({
  saved: null as string | null,
  projects: [] as { id: string }[],
  monos: [] as { id: string; projectIds: string[] }[],
}));
vi.mock("expo-sqlite/kv-store", () => ({
  default: { getItemSync: () => app.saved, setItem: async () => {} },
}));
vi.mock("expo-router", () => ({
  Redirect: ({ href }: { href: { pathname: string; params: Record<string, string> } }) => (
    <output data-redirect={href.pathname} data-params={JSON.stringify(href.params)} />
  ),
  router: { push: vi.fn() },
}));
vi.mock("react-native", () => ({
  View: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  Text: ({ children }: { children?: ReactNode }) => <span>{children}</span>,
  Pressable: ({ children }: { children?: ReactNode }) => <button>{children}</button>,
}));
vi.mock("@/features/connection/ConnectionProvider", () => ({
  useConnection: () => ({ pairing: { desktop: { fingerprint: "desk" } } }),
}));
vi.mock("@/features/connection/ConnectScreen", () => ({
  useConnectionStyles: () => ({ screen: {}, title: {}, subtitle: {}, primary: {}, primaryLabel: {} }),
}));
vi.mock("@/features/sessions/SessionScreen", () => ({
  SessionScreen: ({ children }: { children?: ReactNode }) => <main>{children}</main>,
}));
vi.mock("@/features/workspace/WorkspaceProvider", () => ({
  useWorkspace: () => ({ projects: app.projects, monos: app.monos }),
}));

import Index from "@/app/index";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
let root: ReturnType<typeof createRoot>;
function render() {
  const container = document.createElement("div");
  root = createRoot(container);
  act(() => root.render(<Index />));
  return container;
}
const redirect = (container: HTMLElement) => container.querySelector("[data-redirect]") as HTMLElement | null;

beforeEach(() => {
  app.saved = null;
  app.projects = [{ id: "p1" }, { id: "p2" }];
  app.monos = [{ id: "m1", projectIds: ["p2"] }];
});
afterEach(() => act(() => root.unmount()));

describe("where the app opens", () => {
  it("returns to the last project visited", () => {
    app.saved = JSON.stringify({ kind: "project", id: "p2" });
    const to = redirect(render());
    expect(to?.dataset.redirect).toBe("/project/[projectId]");
    expect(to?.dataset.params).toBe('{"projectId":"p2"}');
  });

  it("opens the first project when nothing was saved", () => {
    expect(redirect(render())?.dataset.params).toBe('{"projectId":"p1"}');
  });

  it("never opens a Mono: a saved Mono place lands on that Mono's project", () => {
    app.saved = JSON.stringify({ kind: "mono", id: "m1" });
    const to = redirect(render());
    expect(to?.dataset.redirect).toBe("/project/[projectId]");
    expect(to?.dataset.params).toBe('{"projectId":"p2"}');
  });

  it("falls back to the first project from a Mono that is gone", () => {
    app.saved = JSON.stringify({ kind: "mono", id: "deleted" });
    expect(redirect(render())?.dataset.params).toBe('{"projectId":"p1"}');
  });

  it("shows the start screen, not a Mono, when no project is on the phone", () => {
    app.projects = [];
    app.saved = JSON.stringify({ kind: "mono", id: "m1" });
    const screen = render();
    expect(redirect(screen)).toBeNull();
    expect(screen.textContent).toContain("Choose the projects you want on your phone.");
    expect(screen.textContent).not.toContain("monos");
  });
});
