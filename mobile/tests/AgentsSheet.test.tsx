import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { AgentsSheet } from "@/features/orchestration/AgentsSheet";
import { OrchestrationStrip } from "@/features/orchestration/OrchestrationStrip";
import type {
  OrchestrationRun,
  OrchestrationTask,
  SessionSummary,
} from "@/features/workspace/types";

type Button = { text: string; style?: string; onPress?: () => void };

const app = vi.hoisted(() => ({
  runs: [] as unknown[],
  read: vi.fn(),
  action: vi.fn(),
  alert: vi.fn(),
  push: vi.fn(),
  back: vi.fn(),
  dismissTo: vi.fn(),
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
    <button aria-label={accessibilityLabel} onClick={onPress} disabled={disabled}>
      {children}
    </button>
  ),
  ScrollView: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  RefreshControl: () => null,
  ActivityIndicator: () => <progress />,
  Alert: { alert: app.alert },
  AppState: { currentState: "active" },
  Platform: { OS: "ios" },
  StyleSheet: { create: (styles: unknown) => styles },
  useColorScheme: () => "dark",
}));
vi.mock("expo-sqlite/kv-store", () => ({
  default: { getItemSync: () => null, setItem: async () => {} },
}));
vi.mock("expo-haptics", () => ({
  impactAsync: async () => {},
  ImpactFeedbackStyle: { Light: "light" },
}));
vi.mock("expo-router", () => ({
  router: { push: app.push, back: app.back, dismissTo: app.dismissTo },
}));
vi.mock("expo-router/react-navigation", () => ({ useHeaderHeight: () => 0 }));
vi.mock("react-native-safe-area-context", () => ({
  useSafeAreaInsets: () => ({ top: 0, bottom: 0, left: 0, right: 0 }),
}));
vi.mock("@/ui/Icon", () => ({ Icon: () => null }));
vi.mock("@/features/sessions/ProviderLogo", () => ({ ProviderLogo: () => null }));
vi.mock("@/features/workspace/WorkspaceProvider", () => ({
  useWorkspace: () => ({
    readOrchestration: app.read,
    orchestrationAction: app.action,
  }),
}));

const task = (
  index: number,
  title: string,
  more: Partial<OrchestrationTask> = {},
): OrchestrationTask => ({
  id: `t${index}`,
  index,
  title,
  prompt: `Do ${title.toLowerCase()}.`,
  files: [`src/${index}.rs`],
  dependsOn: [],
  tone: "queued",
  label: "Queued",
  harness: "claude",
  model: "Sonnet 5.5",
  steps: 0,
  outsideScope: [],
  report: "",
  canRetry: false,
  canRemove: false,
  ...more,
});

const plan = {
  call: "call-1",
  title: "Move sessions to SQLite",
  summary: "Two agents.",
};
const review: OrchestrationRun = {
  ...plan,
  phase: "review",
  headline: "2 assignments to review · not started",
  actions: [
    { action: "start", label: "Start agents", primary: true, disabled: false },
    { action: "dismiss", label: "Dismiss", primary: false, disabled: false },
  ],
  tasks: [
    task(0, "Move the store", { canRemove: true }),
    task(1, "Write the tests", { canRemove: true, dependsOn: ["t0"] }),
  ],
};
const running: OrchestrationRun = {
  ...plan,
  phase: "running",
  headline: "1 of 2 done · 1 needs input · running",
  actions: [{ action: "stop", label: "Stop agents", primary: false, disabled: false }],
  tasks: [
    task(0, "Move the store", {
      tone: "done",
      label: "Done",
      steps: 14,
      report: "Sessions load from SQLite.",
      sessionId: "w0",
    }),
    task(1, "Write the tests", {
      tone: "input",
      label: "Needs input",
      steps: 3,
      sessionId: "w1",
    }),
  ],
};

let host: HTMLDivElement;
let root: Root;

const mount = async (node: ReactNode) => {
  await act(async () => root.render(node));
  await act(async () => {});
};
const button = (name: string) => {
  const found = [...host.querySelectorAll("button")].find(
    (element) =>
      element.getAttribute("aria-label") === name ||
      element.textContent === name,
  );
  if (!found) throw new Error(`No button named ${name}`);
  return found;
};
const press = (name: string) => act(async () => button(name).click());
/** Presses the destructive choice in the confirmation the sheet showed last. */
const confirm = () => {
  const buttons = app.alert.mock.lastCall?.[2] as Button[];
  return act(async () => buttons.find((b) => b.style === "destructive")?.onPress?.());
};

beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  vi.useFakeTimers();
  vi.clearAllMocks();
  app.read.mockImplementation(async () => app.runs);
  app.action.mockImplementation(async () => app.runs);
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.useRealTimers();
});

describe("AgentsSheet", () => {
  it("shows the plan and starts it without asking", async () => {
    app.runs = [review];
    await mount(<AgentsSheet projectId="p" sessionId="lead" />);

    expect(app.read).toHaveBeenCalledWith("lead");
    expect(host.textContent).toContain("2 assignments to review · not started");
    expect(host.textContent).toContain("Move sessions to SQLite");
    expect(host.textContent).toContain("Reply in the chat to revise the plan");

    await press("Start agents");
    expect(app.alert).not.toHaveBeenCalled();
    expect(app.action).toHaveBeenCalledWith("lead", review, "start", undefined);
  });

  it("asks before dismissing the plan or removing an assignment", async () => {
    app.runs = [review];
    await mount(<AgentsSheet projectId="p" sessionId="lead" />);

    await press("Dismiss");
    expect(app.action).not.toHaveBeenCalled();
    await confirm();
    expect(app.action).toHaveBeenLastCalledWith("lead", review, "dismiss", undefined);

    await press("Show details for Write the tests");
    await press("Remove");
    expect(app.alert.mock.lastCall?.[0]).toBe("Remove “Write the tests”?");
    await confirm();
    expect(app.action).toHaveBeenLastCalledWith("lead", review, "remove", review.tasks[1]);
  });

  it("asks before stopping, and opens an agent's chat in place of the sheet", async () => {
    app.runs = [running];
    await mount(<AgentsSheet projectId="p" sessionId="lead" />);

    expect(host.textContent).toContain("Needs input");
    await press("Stop agents");
    expect(app.alert.mock.lastCall?.[0]).toBe("Stop the agents?");
    expect(app.action).not.toHaveBeenCalled();
    await confirm();
    expect(app.action).toHaveBeenCalledWith("lead", running, "stop", undefined);

    await press("Show details for Move the store");
    expect(host.textContent).toContain("Sessions load from SQLite.");
    await press("Open chat");
    expect(app.back).toHaveBeenCalled();
    expect(app.push).toHaveBeenCalledWith({
      pathname: "/project/[projectId]/session/[sessionId]",
      params: { projectId: "p", sessionId: "w0" },
    });
  });

  it("follows the run while it is open", async () => {
    app.runs = [review];
    await mount(<AgentsSheet projectId="p" sessionId="lead" />);
    expect(app.read).toHaveBeenCalledTimes(1);

    app.runs = [running];
    await act(async () => vi.advanceTimersByTime(2000));
    expect(app.read).toHaveBeenCalledTimes(2);
    expect(host.textContent).toContain("1 of 2 done · 1 needs input · running");
  });

  it("says when there is no plan", async () => {
    app.runs = [];
    await mount(<AgentsSheet projectId="p" sessionId="lead" />);
    expect(host.textContent).toContain("No plan right now");
  });
});

describe("OrchestrationStrip", () => {
  const lead = {
    id: "lead",
    projectId: "p",
    title: "Session store refactor",
    orchestration: {
      title: plan.title,
      phase: "running",
      headline: running.headline,
      tones: ["done", "input"],
    },
  } as SessionSummary;

  it("opens the lead's agents", async () => {
    await mount(<OrchestrationStrip session={lead} />);
    expect(host.textContent).toContain(running.headline);
    await press(`Agents: ${plan.title}, ${running.headline}`);
    expect(app.push).toHaveBeenCalledWith({
      pathname: "/agents",
      params: { projectId: "p", sessionId: "lead" },
    });
  });

  it("takes an agent's chat back to its lead", async () => {
    const worker = { id: "w1", projectId: "p", title: "Write the tests" } as SessionSummary;
    await mount(<OrchestrationStrip session={worker} lead={lead} />);
    expect(host.textContent).toContain("Agent of Session store refactor");
    await press("Agent of Session store refactor. Open the lead.");
    expect(app.dismissTo).toHaveBeenCalledWith({
      pathname: "/project/[projectId]/session/[sessionId]",
      params: { projectId: "p", sessionId: "lead" },
    });
  });

  it("draws nothing on a session outside a run", async () => {
    const plain = { id: "s", projectId: "p", title: "Plain" } as SessionSummary;
    await mount(<OrchestrationStrip session={plain} />);
    expect(host.textContent).toBe("");
  });
});
