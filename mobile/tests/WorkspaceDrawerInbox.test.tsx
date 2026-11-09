import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { inboxListRoute } from "@/features/inbox/routing";
import { WorkspaceDrawer } from "@/features/workspace/WorkspaceDrawer";
import { menuLabel } from "@/features/workspace/UnreadDot";

const app = vi.hoisted(() => ({
  push: vi.fn(),
  closeThen: vi.fn((change: () => void) => change()),
  supported: true,
  unread: 3,
  pathname: "/project/one",
  params: { projectId: "one" } as { projectId?: string },
}));

vi.mock("react-native", () => {
  const View = ({ children }: { children?: ReactNode }) => <div>{children}</div>;
  return {
    Image: () => null,
    Platform: { OS: "ios" },
    Pressable: ({ children, onPress, accessibilityLabel }: {
      children: ReactNode; onPress: () => void; accessibilityLabel: string;
    }) => <button onClick={onPress} aria-label={accessibilityLabel}>{children}</button>,
    ScrollView: View,
    StyleSheet: { absoluteFill: {}, create: (styles: unknown) => styles },
    Text: View,
    TextInput: ({ onChangeText, placeholder }: {
      onChangeText: (text: string) => void; placeholder: string;
    }) => <input aria-label={placeholder} onChange={(event) => onChangeText(event.target.value)} />,
    View,
  };
});
vi.mock("@expo/ui/swift-ui", () => ({
  Host: () => null,
  HStack: () => null,
  Label: () => null,
  Menu: () => null,
  Picker: () => null,
  Spacer: () => null,
}));
vi.mock("@expo/ui/swift-ui/modifiers", () => ({
  accessibilityLabel: () => ({}),
  contentShape: () => ({}),
  frame: () => ({}),
  pickerStyle: () => ({}),
  shapes: { circle: () => ({}), rectangle: () => ({}) },
  tag: () => ({}),
}));
vi.mock("expo-router", () => ({
  router: { push: app.push, replace: vi.fn() },
  useGlobalSearchParams: () => app.params,
  usePathname: () => app.pathname,
}));
vi.mock("react-native-reanimated", async () => ({
  default: { View: (await import("react-native")).View },
}));
vi.mock("react-native-safe-area-context", () => ({
  useSafeAreaInsets: () => ({ top: 0, bottom: 0, left: 0, right: 0 }),
}));
vi.mock("@/features/inbox/InboxProvider", () => ({
  useInboxBadge: () => app.unread,
  useInboxSupported: () => app.supported,
}));
vi.mock("@/features/connection/ConnectionProvider", () => ({
  useConnection: () => ({ pairing: { desktop: { fingerprint: "desk" } } }),
  usePairing: () => ({ desktop: { fingerprint: "desk" } }),
  useMonoTools: () => ({ create: vi.fn() }),
}));
vi.mock("@/features/mascots/PixelMascot", () => ({ PixelMascot: () => null }));
vi.mock("@/features/mascots/ProjectMascot", () => ({ ProjectMascot: () => null }));
vi.mock("@/features/sessions/activityStore", () => ({ useFinishedCount: () => 0 }));
vi.mock("@/features/sessions/markdownParser", () => ({ plainText: (text: string) => text }));
vi.mock("@/features/sessions/MonoGroup", () => ({ MonoGroup: () => null }));
vi.mock("@/features/sessions/monoGroupPrefs", () => ({
  setMonoGroupPrefs: vi.fn(),
  useMonoGroupPrefs: () => ({ collapsed: false }),
}));
vi.mock("@/features/sessions/sessionMonos", () => ({
  searchMonos: () => ({ assigned: [] }),
}));
vi.mock("@/features/sessions/SessionScreen", () => ({
  HEADER_GAP: 0,
  HEADER_HEIGHT: 44,
  useKeyboardLift: () => ({}),
}));
vi.mock("@/features/settings/monosInDrawer", () => ({ useMonosInDrawer: () => [false] }));
vi.mock("@/features/workspace/monoSeen", () => ({ useUnreadMonos: () => new Set() }));
vi.mock("@/features/workspace/projectPick", () => ({ pickProject: () => true }));
vi.mock("@/features/workspace/routing", () => ({
  projectForRoute: (projects: { id: string }[], id?: string) =>
    projects.find((project) => project.id === id),
}));
vi.mock("@/features/workspace/useCreateMono", () => ({ useCreateMono: () => vi.fn() }));
vi.mock("@/features/workspace/WorkspaceProvider", () => ({
  useWorkspace: () => ({
    projects: [{ id: "one", name: "One", path: "/one" }],
    monos: [],
  }),
}));
vi.mock("@/ui/AnimationsPaused", () => ({
  AnimationsPaused: ({ children }: { children: ReactNode }) => children,
}));
vi.mock("@/ui/brand", () => ({ MONOCODE_MARK: 1 }));
vi.mock("@/ui/ChoiceMenu", () => ({ ChoiceMenu: () => null }));
vi.mock("@/ui/CircleButton", () => ({ CircleButton: () => null }));
vi.mock("@/ui/DitherLoading", () => ({ useWarmDitherLoading: () => {} }));
vi.mock("@/ui/GlassMenuButton", () => ({ GlassMenuButton: () => null }));
vi.mock("@/ui/GradualBlur", () => ({ GradualBlur: () => null }));
vi.mock("@/ui/Icon", () => ({ Icon: () => null }));
vi.mock("@/ui/Shimmer", () => ({ Shimmer: () => null }));
vi.mock("@/features/workspace/newMono", () => ({ onMonoCreated: () => () => {} }));
vi.mock("@/ui/LiquidView", () => ({
  LiquidView: ({ children }: { children: ReactNode }) => <div>{children}</div>,
}));
vi.mock("@/ui/SceneFade", () => ({ isModalRoute: () => false }));
vi.mock("@/ui/ScenePager", () => ({
  useDrawerActions: () => ({ close: vi.fn(), closeThen: app.closeThen }),
  useDrawerOpen: () => true,
}));
vi.mock("@/ui/theme", () => ({
  radii: { row: 12 },
  themedStyles: (factory: (colors: object) => object) => () => factory({}),
  useColors: () => ({}),
  useScheme: () => "dark",
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const container = document.createElement("div");
let root = createRoot(container);

beforeEach(() => {
  app.supported = true;
  app.unread = 3;
  app.pathname = "/project/one";
  app.params = { projectId: "one" };
  app.push.mockClear();
  app.closeThen.mockClear();
});
afterEach(() => {
  act(() => root.unmount());
  root = createRoot(container);
});

const inboxRow = () => container.querySelector<HTMLButtonElement>('button[aria-label^="Inbox"]');
const render = () => act(() => root.render(<WorkspaceDrawer />));

it("heads the drawer with the unread count", () => {
  render();
  expect(inboxRow()?.getAttribute("aria-label")).toBe("Inbox, 3 unread");
  expect(inboxRow()?.textContent).toContain("3");
});

it("says no count at none", () => {
  app.unread = 0;
  render();
  expect(inboxRow()?.getAttribute("aria-label")).toBe("Inbox");
});

it("shuts the drawer, then pushes the page with the project in front", () => {
  render();
  act(() => inboxRow()!.click());
  expect(app.closeThen).toHaveBeenCalledOnce();
  expect(app.push).toHaveBeenCalledWith({ pathname: "/inbox-list", params: { projectId: "one" } });
});

it("opens with no project off a project's screen", () => {
  app.pathname = "/";
  app.params = {};
  render();
  act(() => inboxRow()!.click());
  expect(app.push).toHaveBeenCalledWith({ pathname: "/inbox-list", params: {} });
});

it("isn't there for a desktop without the Inbox", () => {
  app.supported = false;
  render();
  expect(inboxRow()).toBeNull();
});

it("steps aside while the search narrows the projects", () => {
  render();
  const search = container.querySelector<HTMLInputElement>('input[aria-label="Search"]')!;
  const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
  act(() => {
    set.call(search, "on");
    search.dispatchEvent(new Event("input", { bubbles: true }));
  });
  expect(inboxRow()).toBeNull();
});

it("keeps replies first in what the menu button says", () => {
  expect(menuLabel(true, true)).toBe("Projects and monos, new replies");
  expect(menuLabel(false, true)).toBe("Projects and monos, new in Inbox");
  expect(menuLabel(false, false)).toBe("Projects and monos");
});

it("leaves the project out of the route when there is none", () => {
  expect(inboxListRoute()).toEqual({ pathname: "/inbox-list", params: {} });
});
