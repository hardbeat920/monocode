import { act, useEffect, useState, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";

import { WorkspaceLayout } from "@/features/workspace/WorkspaceLayout";
import { ProjectPages, useProjectPages } from "@/features/workspace/ProjectPages";
import { useContentWidth } from "@/ui/ContentPane";

const app = vi.hoisted(() => ({
  layouts: new Map<string, (event: unknown) => void>(),
  insets: { top: 20, bottom: 16, left: 0, right: 0 },
  navigate: vi.fn(),
  params: { projectId: "/work/project", sessionId: "session-1" },
}));
vi.mock("react-native", () => {
  const View = ({ children, testID, onLayout, style }: {
    children?: ReactNode; testID?: string; onLayout?: (event: unknown) => void; style?: object;
  }) => {
    if (testID && onLayout) app.layouts.set(testID, onLayout);
    return <div data-testid={testID} style={Array.isArray(style) ? Object.assign({}, ...style) : style}>{children}</div>;
  };
  return {
    useWindowDimensions: () => ({ width: 1000, height: 800 }),
    View, ScrollView: View, Text: View,
    Pressable: ({ children, onPress, accessibilityLabel, accessibilityState }: {
      children: ReactNode; onPress: () => void; accessibilityLabel: string; accessibilityState?: { selected: boolean };
    }) => <button onClick={onPress} aria-label={accessibilityLabel} aria-current={accessibilityState?.selected}>{children}</button>,
    StyleSheet: { create: (styles: unknown) => styles, hairlineWidth: 1 },
  };
});
vi.mock("react-native-safe-area-context", () => ({
  SafeAreaProvider: ({ children }: { children: ReactNode }) => children,
  useSafeAreaInsets: () => app.insets,
}));
vi.mock("expo-router", () => ({
  router: { navigate: app.navigate, push: vi.fn() },
  useGlobalSearchParams: () => app.params,
}));
vi.mock("@/ui/theme", () => ({
  useColors: () => ({}),
  themedStyles: (factory: (colors: object) => object) => () => factory({}),
}));
vi.mock("@/ui/Icon", () => ({ Icon: () => null }));
vi.mock("@/features/mascots/ProjectMascot", () => ({ ProjectMascot: () => null }));
vi.mock("@/features/sessions/ProviderLogo", () => ({ ProviderLogo: () => null }));
vi.mock("@/features/workspace/WorkspaceProvider", () => ({
  useWorkspace: () => ({
    projects: [{ id: "encoded-project", path: "/work/project", name: "Project" }],
    monos: [],
    sessions: [
      { id: "session-1", projectId: "encoded-project", title: "First session", updatedAt: 1 },
      { id: "session-2", projectId: "encoded-project", title: "Latest session", updatedAt: 2 },
      { id: "hidden", projectId: "unselected-project", title: "Hidden session", updatedAt: 3 },
    ],
  }),
}));
vi.mock("@/ui/ScenePager", () => ({ useScenePager: () => ({ get: () => 1 }) }));
vi.mock("@/ui/SegmentedSwitch", () => ({ SegmentedSwitch: () => null }));
vi.mock("react-native-reanimated", async () => ({
  default: { View: (await import("react-native")).View },
  useAnimatedStyle: (factory: () => object) => factory(),
}));

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const container = document.createElement("div");
let root = createRoot(container);
afterEach(() => {
  act(() => root.unmount());
  root = createRoot(container);
  app.layouts.clear();
  app.navigate.mockClear();
  app.insets.left = app.insets.right = 0;
});
function resize(rootWidth: number, paneWidth: number) {
  act(() => app.layouts.get("workspace-layout")!({ nativeEvent: { layout: { width: rootWidth } } }));
  act(() => app.layouts.get("content-pane")!({ nativeEvent: { layout: { width: paneWidth } } }));
}

it("preserves React child identity and draft state across compact/expanded transitions", () => {
  const mounted = vi.fn();
  const disposed = vi.fn();
  function Conversation() {
    const [draft, setDraft] = useState("");
    const width = useContentWidth();
    useEffect(() => { mounted(); return disposed; }, []);
    return <>
      <button onClick={() => setDraft("Keep my draft")}>Draft</button>
      <input readOnly value={draft} />
      <div data-transcript data-width={width}>Conversation</div>
    </>;
  }
  act(() => root.render(<WorkspaceLayout><Conversation /></WorkspaceLayout>));
  resize(400, 400);
  expect(container.querySelector('[data-testid="workspace-sidebar"]')).toBeNull();
  act(() => container.querySelector("button")!.click());
  const transcript = container.querySelector<HTMLElement>("[data-transcript]")!;
  for (const [width, pane] of [[1000, 740], [400, 400], [1000, 740]]) {
    resize(width, pane);
    expect(container.querySelector("[data-transcript]")).toBe(transcript);
    expect(transcript.dataset.width).toBe(String(pane));
    expect(container.querySelector("input")!.value).toBe("Keep my draft");
  }
  expect(mounted).toHaveBeenCalledTimes(1);
  expect(disposed).not.toHaveBeenCalled();
});

it("requires room for both panes after horizontal safe areas", () => {
  app.insets.left = 30;
  app.insets.right = 40;
  act(() => root.render(<WorkspaceLayout><span>Chat</span></WorkspaceLayout>));
  resize(809, 809);
  expect(container.querySelector('[data-testid="workspace-sidebar"]')).toBeNull();
  resize(810, 520);
  expect(container.querySelector('[data-testid="workspace-sidebar"]')).not.toBeNull();
});

it("opens canonical project/session routes and shows selected, newest sessions first", () => {
  act(() => root.render(<WorkspaceLayout><span>Chat</span></WorkspaceLayout>));
  resize(1000, 740);
  const project = container.querySelector<HTMLButtonElement>('[aria-label="Open project Project"]')!;
  expect(project.getAttribute("aria-current")).toBe("true");
  act(() => project.click());
  expect(app.navigate).toHaveBeenLastCalledWith({ pathname: "/project/[projectId]", params: { projectId: "encoded-project" } });
  const sessions = container.querySelectorAll<HTMLButtonElement>('[aria-label^="Open session"]');
  expect(sessions).toHaveLength(2);
  expect(sessions[0].textContent).toContain("Latest session");
  act(() => sessions[0].click());
  expect(app.navigate).toHaveBeenLastCalledWith({ pathname: "/project/[projectId]/session/[sessionId]", params: { projectId: "encoded-project", sessionId: "session-2" } });
  app.navigate.mockClear();
  act(() => sessions[1].click());
  expect(app.navigate).not.toHaveBeenCalled();
});

it("sizes and offsets project pages by the measured content pane", () => {
  function Pages() {
    const pages = useProjectPages("project");
    return <ProjectPages pages={pages} chat={<span>Chat</span>} sessions={<span>Sessions</span>} />;
  }
  act(() => root.render(<WorkspaceLayout><Pages /></WorkspaceLayout>));
  resize(1000, 740);
  const pane = container.querySelector('[data-testid="content-pane"]')!;
  const pages = pane.firstElementChild as HTMLElement;
  expect(pages.style.width).toBe("1480px");
  expect((pages.firstElementChild as HTMLElement).style.width).toBe("740px");
  resize(400, 400);
  expect(pages.style.width).toBe("800px");
  expect((pages.firstElementChild as HTMLElement).style.width).toBe("400px");
});

it("uses window width before measurement, then the local pane width", () => {
  function Width() { return <span data-width={useContentWidth()} />; }
  act(() => root.render(<WorkspaceLayout><Width /></WorkspaceLayout>));
  expect(container.querySelector("[data-width]")!.getAttribute("data-width")).toBe("1000");
  resize(1000, 740);
  expect(container.querySelector("[data-width]")!.getAttribute("data-width")).toBe("740");
});
