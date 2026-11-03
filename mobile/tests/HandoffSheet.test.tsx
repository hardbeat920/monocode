import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { HandoffProvider, useHandoff } from "@/features/sessions/HandoffSheet";
import {
  useForgetChoiceOnLeave,
  useSessionChoice,
} from "@/features/models/sessionChoice";
import type { ModelCatalog } from "@/features/models/types";
import type { Project } from "@/features/workspace/types";

const runtime = vi.hoisted(() => ({
  projects: [{ id: "project", path: "/repo" }] as Project[],
  onDismiss: () => {},
  push: vi.fn(),
  alert: vi.fn(),
  create: vi.fn(),
}));
vi.mock("react-native", () => ({
  Platform: { OS: "ios" },
  Keyboard: { dismiss: vi.fn() },
  Alert: { alert: runtime.alert },
  View: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  Text: ({ children }: { children?: ReactNode }) => <span>{children}</span>,
  ScrollView: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  Pressable: ({
    children,
    onPress,
    disabled,
  }: {
    children?: ReactNode;
    onPress: () => void;
    disabled?: boolean;
  }) => (
    <button onClick={onPress} disabled={disabled}>
      {children}
    </button>
  ),
  ActivityIndicator: () => null,
  Modal: ({
    children,
    visible,
    onDismiss,
  }: {
    children?: ReactNode;
    visible: boolean;
    onDismiss: () => void;
  }) => {
    runtime.onDismiss = onDismiss;
    return visible ? <section>{children}</section> : null;
  },
}));
vi.mock("react-native-safe-area-context", () => ({
  useSafeAreaInsets: () => ({ top: 0, bottom: 0 }),
}));
vi.mock("expo-router", () => ({ router: { push: runtime.push } }));
vi.mock("@/features/connection/ConnectionProvider", () => ({
  useConnection: () => ({ pairing: { desktop: { fingerprint: "desktop" } } }),
}));
vi.mock("@/features/settings/defaultModel", () => ({
  useDefaultModel: () => null,
  useDefaultRuntimeMode: () => null,
}));
vi.mock("@/features/workspace/WorkspaceProvider", () => ({
  useWorkspace: () => ({
    allProjects: runtime.projects,
    sessions: [],
    listModels: async () => catalog,
    handoffSession: runtime.create,
    configureSession: vi.fn(),
  }),
}));
vi.mock("@/features/sessions/ProviderLogo", () => ({
  ProviderLogo: () => null,
}));
vi.mock("@/ui/Icon", () => ({ Icon: () => null }));
vi.mock("@/ui/Shimmer", () => ({ Shimmer: () => null }));
vi.mock("@/ui/theme", () => ({
  useColors: () => ({}),
  themedStyles: () => () => ({}),
}));

const project = { id: "project", path: "/repo" } as Project;
const catalog: ModelCatalog = {
  harnesses: [{ id: "grok", label: "Grok" }],
  models: [
    {
      id: "grok",
      harness: "grok",
      name: "Grok 4.7",
      settings: [
        {
          id: "effort",
          label: "Effort",
          kind: "select",
          value: "high",
          options: [
            { value: "high", label: "High" },
            { value: "low", label: "Low" },
          ],
        },
      ],
    },
  ],
  current: {
    harness: "codex",
    model: "sol",
    modelSettings: {},
    runtimeMode: "supervised",
  },
  runtimeModes: [],
};
let choice: ReturnType<typeof useSessionChoice>;
function Probe() {
  const show = useHandoff();
  choice = useSessionChoice(project, "created");
  useForgetChoiceOnLeave(project.id, "created");
  return <button onClick={() => show?.("reply", "codex")}>Handoff</button>;
}
function App() {
  return (
    <HandoffProvider projectId="project" conversationId="source">
      <Probe />
    </HandoffProvider>
  );
}
let root: ReturnType<typeof createRoot>;
let container: HTMLDivElement;
beforeEach(() => {
  (
    globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }
  ).IS_REACT_ACT_ENVIRONMENT = true;
  vi.clearAllMocks();
  runtime.projects = [project];
  runtime.create.mockResolvedValue({ sessionId: "created" });
  container = document.createElement("div");
  root = createRoot(container);
  act(() => root.render(<App />));
});
afterEach(() => act(() => root.unmount()));
async function click(text: string) {
  const button = [...container.querySelectorAll("button")].find(
    (button) => button.textContent === text,
  )!;
  expect(button).toBeDefined();
  await act(async () => button.click());
}
async function createHandoff() {
  await click("Handoff");
  await click("Grok 4.7");
  await click("Low");
  await click("Create handoff");
}
it("remembers the selected effort before catalog refresh and opens only after iOS dismissal", async () => {
  await createHandoff();
  expect(choice.chosen).toEqual({
    name: "Grok 4.7",
    effort: "Low",
    pick: { harness: "grok", model: "grok", modelSettings: { effort: "low" } },
  });
  expect(runtime.push).not.toHaveBeenCalled();
  act(() => runtime.onDismiss());
  expect(runtime.push).toHaveBeenCalledWith({
    pathname: "/session",
    params: {
      projectId: "project",
      sessionId: "created",
      focusComposer: "true",
    },
  });
  act(() => runtime.onDismiss());
  expect(runtime.push).toHaveBeenCalledOnce();
});
it("clears the remembered choice when the project disappears before iOS dismissal", async () => {
  await createHandoff();
  runtime.projects = [];
  act(() => root.render(<App />));
  act(() => runtime.onDismiss());
  expect(runtime.push).not.toHaveBeenCalled();
  expect(runtime.alert).toHaveBeenCalledOnce();
  expect(choice.chosen).toBeNull();
});
