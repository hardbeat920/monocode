import { act, useState, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { ProjectScreen } from "@/features/workspace/ProjectScreen";
import type { Project } from "@/features/workspace/types";
import type { Workspace } from "@/features/workspace/WorkspaceMenu";

const desktop = vi.hoisted(() => ({
  startSession: vi.fn(),
  sendMessage: vi.fn(),
  offline: false,
  existing: { path: "/repo/.worktrees/mobile", name: "feature/mobile" },
}));

vi.mock("react-native", () => ({
  View: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  Text: ({ children }: { children?: ReactNode }) => <span>{children}</span>,
  Pressable: ({
    children,
    onPress,
    accessibilityLabel,
    disabled,
  }: {
    children?: ReactNode;
    onPress?: () => void;
    accessibilityLabel?: string;
    disabled?: boolean;
  }) => (
    <button
      aria-label={accessibilityLabel}
      onClick={onPress}
      disabled={disabled}
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
  Stack: {
    Screen: () => null,
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
  SessionScreen: ({
    below,
    composer,
  }: {
    below?: ReactNode;
    composer?: ReactNode;
  }) => (
    <>
      <header>{below}</header>
      {composer}
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
  }),
  useConversation: () => ({}),
  useConversationTurns: () => ({}),
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
vi.mock("@/ui/Icon", () => ({ Icon: () => null }));
vi.mock("@/ui/theme", () => {
  const colors = { content: "white", muted: "gray", fill: "gray" };
  return {
    useColors: () => colors,
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
    container = document.createElement("div");
    root = createRoot(container);
  });
  afterEach(() => act(() => root.unmount()));

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
