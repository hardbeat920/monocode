import { act, useState, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { ProjectScreen } from "@/features/workspace/ProjectScreen";
import MonoScreen from "@/app/mono/[monoId]";
import type { UsageLimitState, Project } from "@/features/workspace/types";
import type { Workspace } from "@/features/workspace/WorkspaceMenu";
import { GlassBlurTarget } from "@/ui/LiquidView";

const desktop = vi.hoisted(() => ({
  startSession: vi.fn(),
  sendMessage: vi.fn(),
  offline: false,
  os: "ios",
  usageLimit: undefined as UsageLimitState | undefined,
  cardMounts: 0,
  monoId: "first",
  watched: null as string | null,
  headerTarget: undefined as unknown,
  header: {} as {
    headerTitle?: unknown;
    headerLeft?: unknown;
    headerRight?: unknown;
    headerTitleAlign?: string;
    headerBackVisible?: boolean;
  },
  existing: { path: "/repo/.worktrees/mobile", name: "feature/mobile" },
}));

vi.mock("@/features/sessions/UsageLimitCard", () => ({
  UsageLimitCard: ({ conversationId, state }: { conversationId: string; state: UsageLimitState }) => {
    const [mount] = useState(() => ++desktop.cardMounts);
    return <output data-usage-card data-mount={mount}>{conversationId}:{state.token}</output>;
  },
}));
vi.mock("@/features/mascots/PixelMascot", () => ({ PixelMascot: () => null }));
vi.mock("expo-haptics", () => ({}));
vi.mock("@/features/connection/ConnectionProvider", () => ({
  useConnection: () => ({}),
  useMonoTools: () => false,
}));
vi.mock("@/features/models/useMonoModel", () => ({ useMonoModel: () => ({}) }));
vi.mock("@/features/workspace/monoAccent", () => ({ useMonoAccent: () => "theme" }));
vi.mock("@/features/workspace/lastPlace", () => ({ rememberPlace: vi.fn() }));
vi.mock("@/features/workspace/monoSeen", () => ({ markMonoSeen: vi.fn(), markMonoUnread: vi.fn() }));
vi.mock("@/features/sessions/MonoCards", () => ({
  MonoChoose: ({ children }: { children: ReactNode }) => children,
  MonoMessageActions: ({ children }: { children: ReactNode }) => children,
}));
vi.mock("@/features/sessions/MonoMenu", () => ({ MonoMenu: () => null }));
vi.mock("@/ui/Timed", () => ({ Timed: ({ children }: { children: ReactNode }) => children }));
vi.mock("react-native-svg", () => ({ default: () => <svg />, Path: () => null }));
vi.mock("react-native", () => ({
  Platform: {
    get OS() {
      return desktop.os;
    },
  },
  View: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  Text: ({ children }: { children?: ReactNode }) => <span>{children}</span>,
  Pressable: ({
    children,
    onPress,
    accessibilityLabel,
    disabled,
    style,
  }: {
    children?: ReactNode;
    onPress?: () => void;
    accessibilityLabel?: string;
    disabled?: boolean;
    style?: object | object[];
  }) => (
    <button
      aria-label={accessibilityLabel}
      onClick={onPress}
      disabled={disabled}
      style={
        typeof style === "function"
          ? undefined
          : Array.isArray(style)
            ? Object.assign({}, ...style)
            : style
      }
    >
      {children}
    </button>
  ),
  StyleSheet: { create: (styles: unknown) => styles },
}));
vi.mock("react-native-reanimated", () => ({
  default: {
    View: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  },
  Easing: { out: () => {}, cubic: () => {} },
  useAnimatedStyle: () => ({}),
  useDerivedValue: (read: () => unknown) => ({ get: read }),
  useSharedValue: (initial: unknown) =>
    useState(() => {
      let value = initial;
      return {
        get: () => value,
        set: (next: unknown) => {
          value = next;
        },
      };
    })[0],
  withTiming: (value: unknown) => value,
}));
vi.mock("react-native-worklets", () => ({ scheduleOnRN: vi.fn() }));
vi.mock("expo-router", () => ({
  router: { push: vi.fn() },
  Redirect: () => null,
  useLocalSearchParams: () => ({ monoId: desktop.monoId }),
  Stack: {
    Screen: ({ options }: { options: typeof desktop.header }) => {
      desktop.header = options;
      if (desktop.os !== "android") return null;
      return (
        <>
          {typeof options.headerLeft === "function"
            ? options.headerLeft()
            : null}
          {typeof options.headerTitle === "function"
            ? options.headerTitle()
            : null}
          {typeof options.headerRight === "function"
            ? options.headerRight()
            : null}
        </>
      );
    },
    Toolbar: Object.assign(() => null, { View: () => null }),
  },
  useIsFocused: () => true,
}));
vi.mock("@/features/connection/ConnectionNotice", () => ({
  useOffline: () => desktop.offline,
  ConnectionNotice: () => null,
  ReconnectingStatus: () => null,
}));
vi.mock("@/features/connection/nativeLink", () => ({
  hasNativeTranscript: false,
}));
vi.mock("@/features/connection/useProjectChanges", () => ({
  useProjectChanges: (_project: string, _enabled: boolean, path?: string) => ({
    result: { branch: path ? desktop.existing.name : "main", files: [] },
    error: null,
  }),
}));
vi.mock("@/features/connection/useProjectWorktrees", () => ({
  useProjectWorktrees: () => ({ worktrees: [desktop.existing], loaded: true }),
}));
vi.mock("@/features/background/useEmptyBackground", () => ({
  useEmptyBackground: vi.fn(),
}));
vi.mock("@/features/mascots/ProjectMascot", () => ({
  ProjectMascot: () => null,
}));
vi.mock("@/features/models/sessionChoice", () => ({
  useSessionChoice: (_project: Project, sessionId?: string) => ({
    session: sessionId ? { id: sessionId, branch: "main" } : undefined,
    accounts: {},
  }),
  useForgetChoiceOnLeave: () => {},
}));
vi.mock("@/features/models/useModelCatalog", () => ({
  useModelCatalog: () => ({}),
}));
vi.mock("@/features/settings/newSessions", () => ({
  useOpenSessionAfterStarting: () => [false],
}));
vi.mock("@/features/sessions/Composer", () => ({
  Composer: ({
    workspace,
    onSend,
  }: {
    workspace?: ReactNode;
    onSend: (text: string, attachments: never[]) => Promise<void>;
  }) => (
    <section data-composer>
      {workspace}
      <button onClick={() => void onSend("Build the app", [])}>Send</button>
    </section>
  ),
}));
vi.mock("@/features/sessions/EmptySession", () => ({
  EmptySession: () => null,
}));
vi.mock("@/features/sessions/sessionError", () => ({
  useSessionErrorNotice: () => null,
  useReportSessionError: () => {},
  latestSessionError: () => null,
}));
vi.mock("@/features/sessions/LiveSessions", () => ({
  LiveSessions: () => null,
}));
vi.mock("@/features/sessions/NativeTranscript", () => ({
  NativeTranscript: () => null,
}));
vi.mock("@/features/sessions/Markdown", () => ({ FileOpener: () => null }));
vi.mock("@/features/sessions/activityStore", () => ({
  markStartedHere: () => {},
  markViewing: () => () => {},
}));
vi.mock("@/features/sessions/SessionScreen", () => ({
  SessionContent: ({ children }: { children?: ReactNode }) => <>{children}</>,
  SessionScreen: ({
    below,
    composer,
    children,
  }: {
    below?: ReactNode;
    composer?: ReactNode;
    children?: ReactNode;
  }) => (
    <>
      <header>{below}</header>
      {composer}
      {children}
    </>
  ),
}));
vi.mock("@/features/sessions/SessionsPage", () => ({
  SessionsPage: () => null,
}));
vi.mock("@/features/sessions/SessionToolbar", () => ({
  SessionTitle: () => null,
  SessionToolbar: () => null,
}));
vi.mock("@/features/sessions/Transcript", () => ({
  Transcript: () => null,
  TranscriptLoading: () => null,
}));
vi.mock("@/features/workspace/ProjectPages", () => ({
  useProjectPages: () => ({ progress: { get: () => 0 }, view: "chat" }),
  ProjectPages: () => null,
  ProjectSwitch: () => null,
}));
vi.mock("@/features/workspace/WorkspaceProvider", () => ({
  useWorkspace: () => ({
    startSession: desktop.startSession,
    sendMessage: desktop.sendMessage,
    monos: ["first", "second"].map(id => ({ id, name: id, mascot: "cat", color: "blue", status: "idle" })),
  }),
  monoConversationId: (id: string) => `mono:${id}`,
  useConversation: (id: string | null) => { desktop.watched = id; return { usageLimit: desktop.usageLimit }; },
  useConversationTurns: () => ({ turns: [], hasOlder: false }),
}));
vi.mock("@/features/workspace/WorkspaceMenu", () => ({
  WorkspaceMenu: ({
    value,
    onChange,
  }: {
    value: Workspace;
    onChange: (value: Workspace) => void;
  }) => (
    <>
      <output data-workspace>{value.mode}</output>
      <button onClick={() => onChange({ mode: "worktree" })}>
        Header: new worktree
      </button>
      <button
        onClick={() =>
          onChange({ mode: "existing", worktree: desktop.existing })
        }
      >
        Header: existing worktree
      </button>
      <button onClick={() => onChange({ mode: "current" })}>
        Header: current checkout
      </button>
    </>
  ),
  WorkspaceLine: ({ branch }: { branch: string }) => <span>{branch}</span>,
}));
// The drawer's pager and its loading cover reach native modules; here the
// drawer is shut and no screen switch is under way.
vi.mock("@/ui/ScenePager", () => ({
  useDrawerActions: () => ({
    open: vi.fn(),
    close: vi.fn(),
    closeThen: (change: () => void) => change(),
    allowSwipe: () => () => {},
  }),
  useDrawerSwipe: vi.fn(),
  useSceneSwapping: () => false,
  useSceneSwitching: () => false,
}));
vi.mock("@/ui/DitherLoading", () => ({ DitherLoading: () => null }));
vi.mock("@/features/workspace/UnreadDot", () => ({
  UnreadDot: () => null,
  useAnyUnreadMono: () => false,
}));
vi.mock("@/ui/Icon", () => ({ Icon: () => null }));
vi.mock("@/ui/LiquidView", async () => {
  const { createContext, use } = await import("react");
  const targetContext = createContext<unknown>(undefined);
  return {
    GlassBlurTarget: targetContext,
    LiquidView: ({
      children,
      accessibilityLabel,
      onPress,
      borderRadius,
      innerStyle,
    }: {
      children?: ReactNode;
      accessibilityLabel?: string;
      onPress?: () => void;
      borderRadius?: number;
      innerStyle?: object;
    }) => {
      desktop.headerTarget = use(targetContext);
      return (
        <button
          data-glass
          aria-label={accessibilityLabel}
          onClick={onPress}
          style={{ ...innerStyle, borderRadius }}
        >
          {children}
        </button>
      );
    },
  };
});
vi.mock("@/ui/theme", () => {
  const colors = { content: "white", muted: "gray", fill: "gray" };
  return {
    useColors: () => colors,
    useScheme: () => "dark",
    ACCENTS: {},
    AccentOverride: ({ children }: { children: ReactNode }) => children,
    themedStyles: (create: (value: typeof colors) => object) => () =>
      create(colors),
  };
});
vi.mock("@/ui/usePushedIn", () => ({
  usePushedIn: () => true,
  usePushStarted: () => true,
}));

(
  globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }
).IS_REACT_ACT_ENVIRONMENT = true;

const project: Project = {
  id: "project",
  name: "Monocode",
  path: "/repo",
  machine: "desktop",
  branch: "main",
  color: "blue",
  additions: 0,
  deletions: 0,
};

describe("new session composer workspace", () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  const render = (sessionId?: string) =>
    act(() =>
      root.render(<ProjectScreen project={project} sessionId={sessionId} />),
    );
  const button = (label: string) => {
    const found = Array.from(container.querySelectorAll("button")).find(
      (button) =>
        button.getAttribute("aria-label") === label ||
        button.textContent === label,
    );
    expect(found, label).toBeDefined();
    return found!;
  };
  const click = (label: string) => act(() => button(label).click());
  const send = () =>
    act(async () => {
      button("Send").click();
      await Promise.resolve();
    });
  beforeEach(() => {
    desktop.startSession.mockReset().mockResolvedValue("started-session");
    desktop.sendMessage.mockReset().mockResolvedValue(undefined);
    desktop.offline = false;
    desktop.os = "ios";
    desktop.usageLimit = undefined;
    desktop.cardMounts = 0;
    desktop.monoId = "first";
    container = document.createElement("div");
    root = createRoot(container);
  });
  afterEach(() => act(() => root.unmount()));

  const usageLimit: UsageLimitState = { token: "paused", canResume: true, canAutoResume: false, resumeAtReset: false,
    resetLabel: "Reset time unknown", limitation: "Keep desktop running" };
  it("wires usage cards into project sessions and remounts them when the session changes", () => {
    desktop.usageLimit = usageLimit;
    render("first");
    expect(container.querySelector("[data-usage-card]")?.textContent).toBe("first:paused");
    expect(desktop.watched).toBe("first");
    render("second");
    expect(container.querySelector("[data-usage-card]")?.textContent).toBe("second:paused");
    expect(desktop.cardMounts).toBe(2);
    desktop.usageLimit = undefined;
    render("second");
    expect(container.querySelector("[data-usage-card]")).toBeNull();
  });
  it("wires usage cards into Mono sessions and remounts them when the Mono changes", () => {
    desktop.usageLimit = usageLimit;
    act(() => root.render(<MonoScreen />));
    expect(container.querySelector("[data-usage-card]")?.textContent).toBe("mono:first:paused");
    expect(desktop.watched).toBe("mono:first");
    desktop.monoId = "second";
    act(() => root.render(<MonoScreen />));
    expect(container.querySelector("[data-usage-card]")?.textContent).toBe("mono:second:paused");
    expect(desktop.cardMounts).toBe(2);
  });

  it("keeps circular Android project/settings controls after switching projects", () => {
    desktop.os = "android";
    render();
    // The drawer's button, where the project switcher was.
    for (const label of ["Projects and monos", "Settings"]) {
      expect(button(label).style.borderRadius).toBe("22px");
      expect(button(label).hasAttribute("data-glass")).toBe(true);
    }
    act(() =>
      root.render(
        <ProjectScreen
          project={{ ...project, id: "other", name: "Other" }}
          sessionId={undefined}
        />,
      ),
    );
    expect(button("Projects and monos").style.borderRadius).toBe("22px");
    expect(button("Settings").hasAttribute("data-glass")).toBe(true);
    expect(desktop.header.headerTitleAlign).toBe("center");
  });

  it("carries the page blur target into header callbacks rendered outside the screen", () => {
    desktop.os = "android";
    const target = { current: null };
    act(() =>
      root.render(
        <GlassBlurTarget value={target}>
          <ProjectScreen project={project} sessionId={undefined} />
        </GlassBlurTarget>,
      ),
    );
    const renderAvatar = desktop.header.headerLeft as () => ReactNode;
    act(() => root.render(<>{renderAvatar()}</>));
    expect(desktop.headerTarget).toBe(target);
    expect(button("Projects and monos").hasAttribute("data-glass")).toBe(true);
  });

  it("keeps Android session titles next to the back target and actions on the right", () => {
    desktop.os = "android";
    render("existing-session");
    expect(desktop.header.headerTitleAlign).toBe("left");
    expect(desktop.header.headerBackVisible).toBe(false);
    expect(desktop.header.headerTitle).toBeTypeOf("function");
    expect(desktop.header.headerRight).toBeTypeOf("function");
  });

  it("switches to a new worktree from the composer and sends that choice", async () => {
    render();
    click("Current checkout");
    expect(button("New worktree")).toBeDefined();
    expect(container.querySelector("[data-workspace]")?.textContent).toBe(
      "worktree",
    );
    expect(desktop.startSession).not.toHaveBeenCalled();
    await send();
    expect(desktop.startSession).toHaveBeenCalledWith(
      project.id,
      "Build the app",
      { workspaceMode: "worktree" },
      [],
    );
  });

  it("reflects the header choice and can switch back to the checkout before sending", async () => {
    render();
    click("Header: new worktree");
    click("New worktree");
    expect(container.querySelector("[data-workspace]")?.textContent).toBe(
      "current",
    );
    await send();
    expect(desktop.startSession).toHaveBeenCalledWith(
      project.id,
      "Build the app",
      {},
      [],
    );
  });

  it("keeps an existing worktree chosen in the header and sends its exact path", async () => {
    render();
    click("Header: existing worktree");
    expect(container.querySelector("[data-composer]")?.textContent).toContain(
      desktop.existing.name,
    );
    expect(
      container.querySelector('[aria-label="Current checkout"]'),
    ).toBeNull();
    await send();
    expect(desktop.startSession).toHaveBeenCalledWith(
      project.id,
      "Build the app",
      { worktreePath: desktop.existing.path },
      [],
    );
    click("Header: current checkout");
    expect(button("Current checkout")).toBeDefined();
  });

  it("offers the toggle only for new drafts and disables it while offline", async () => {
    desktop.offline = true;
    render();
    expect(button("Current checkout").disabled).toBe(true);
    click("Current checkout");
    expect(container.querySelector('[aria-label="New worktree"]')).toBeNull();
    desktop.offline = false;
    render("existing-session");
    expect(
      container.querySelector('[aria-label="Current checkout"]'),
    ).toBeNull();
    expect(container.querySelector('[aria-label="New worktree"]')).toBeNull();
    await send();
    expect(desktop.sendMessage).toHaveBeenCalledWith(
      "existing-session",
      "Build the app",
      [],
    );
    expect(desktop.startSession).not.toHaveBeenCalled();
  });
});
