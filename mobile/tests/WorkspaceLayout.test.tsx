import { act, type ReactNode, useState } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { WorkspaceLayout } from "@/features/workspace/WorkspaceLayout";
import { useWorkspaceSidebar } from "@/features/workspace/WorkspaceSidebarContext";
import { usePaneWidth } from "@/ui/PaneWidth";

const app = vi.hoisted(() => ({
  width: 1180,
  navigate: vi.fn(),
  sessionId: "one",
}));
vi.mock("expo-router", () => ({
  router: { navigate: app.navigate },
  useGlobalSearchParams: () => ({
    projectId: "project",
    sessionId: app.sessionId,
  }),
}));
vi.mock("react-native", () => ({
  useWindowDimensions: () => ({ width: app.width }),
  StyleSheet: { hairlineWidth: 1 },
  View: ({
    children,
    accessibilityLabel,
  }: {
    children: ReactNode;
    accessibilityLabel?: string;
  }) => <div aria-label={accessibilityLabel}>{children}</div>,
  Text: ({ children }: { children: ReactNode }) => <span>{children}</span>,
  ScrollView: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  Pressable: ({
    children,
    onPress,
    accessibilityLabel,
    accessibilityState,
  }: {
    children: ReactNode;
    onPress: () => void;
    accessibilityLabel: string;
    accessibilityState?: { selected: boolean };
  }) => (
    <button
      onClick={onPress}
      aria-label={accessibilityLabel}
      aria-selected={accessibilityState?.selected}
    >
      {children}
    </button>
  ),
}));
vi.mock("react-native-safe-area-context", () => ({
  useSafeAreaInsets: () => ({ top: 24, left: 0, bottom: 20 }),
}));
vi.mock("@/ui/theme", () => ({
  themedStyles: (build: (colors: object) => object) => () => build({}),
}));
vi.mock("@/ui/Icon", () => ({ Icon: () => null }));
vi.mock("@/features/mascots/ProjectMascot", () => ({
  ProjectMascot: () => null,
}));
vi.mock("@/features/sessions/SessionScreen", () => ({
  SheetLayout: ({ children }: { children: ReactNode }) => children,
}));
vi.mock("@/features/workspace/WorkspaceProvider", () => ({
  useWorkspace: () => ({
    projects: [
      { id: "project", path: "/repo", name: "Repo" },
      { id: "other", path: "/other", name: "Other" },
    ],
  }),
}));
vi.mock("@/features/sessions/SessionsPage", () => ({
  SessionsPage: ({
    selectedId,
    onOpen,
  }: {
    selectedId: string;
    onOpen: (session: { id: string }) => void;
  }) => (
    <>
      {["one", "two"].map((id) => (
        <button
          key={id}
          aria-selected={selectedId === id}
          onClick={() => onOpen({ id })}
        >
          {id}
        </button>
      ))}
    </>
  ),
}));
function Conversation() {
  const [draft, setDraft] = useState("draft to preserve");
  const sidebar = useWorkspaceSidebar();
  return (
    <>
      <input value={draft} onChange={(event) => setDraft(event.target.value)} />
      <span data-width>{usePaneWidth()}</span>
      {sidebar.wide && <button onClick={sidebar.toggle}>Toggle sidebar</button>}
    </>
  );
}
const container = document.createElement("div");
const root = createRoot(container);
(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
afterEach(() => {
  act(() => root.render(null));
  app.width = 1180;
  app.navigate.mockClear();
});
const render = () =>
  act(() =>
    root.render(
      <WorkspaceLayout>
        <Conversation />
      </WorkspaceLayout>,
    ),
  );
const click = (label: string) =>
  act(() =>
    Array.from(container.querySelectorAll("button"))
      .find((button) => button.textContent === label)!
      .click(),
  );

it("keeps the conversation and draft mounted when collapsing and resizing", () => {
  render();
  const input = container.querySelector("input");
  expect(container.querySelector("[data-width]")?.textContent).toBe("860");
  click("Toggle sidebar");
  expect(
    container.querySelector('[aria-label="Workspace sidebar"]'),
  ).toBeNull();
  expect(container.querySelector("[data-width]")?.textContent).toBe("1180");
  expect(container.querySelector("input")).toBe(input);
  click("Toggle sidebar");
  app.width = 600;
  render();
  expect(
    container.querySelector('[aria-label="Workspace sidebar"]'),
  ).toBeNull();
  expect(container.querySelector("[data-width]")?.textContent).toBe("600");
  expect(container.querySelector("input")).toBe(input);
  expect(input?.value).toBe("draft to preserve");
  app.width = 1180;
  render();
  expect(
    container.querySelector('[aria-label="Workspace sidebar"]'),
  ).not.toBeNull();
});

it("selects projects and sessions through routes and ignores the selected session", () => {
  render();
  expect(
    container
      .querySelector('button[aria-label="Repo"]')
      ?.getAttribute("aria-selected"),
  ).toBe("true");
  expect(
    Array.from(container.querySelectorAll("button"))
      .find((button) => button.textContent === "one")
      ?.getAttribute("aria-selected"),
  ).toBe("true");
  click("one");
  expect(app.navigate).not.toHaveBeenCalled();
  click("two");
  expect(app.navigate).toHaveBeenLastCalledWith({
    pathname: "/project/[projectId]/session/[sessionId]",
    params: { projectId: "project", sessionId: "two" },
  });
  act(() =>
    (
      container.querySelector('button[aria-label="Other"]') as HTMLButtonElement
    ).click(),
  );
  expect(app.navigate).toHaveBeenLastCalledWith({
    pathname: "/project/[projectId]",
    params: { projectId: "other" },
  });
});
