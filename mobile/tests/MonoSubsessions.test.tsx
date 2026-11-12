import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import MonoSubsessionsScreen from "@/app/mono-profile/[monoId]/subsessions";
import type { MonoSubsessions } from "@/features/workspace/types";

const desktop = vi.hoisted(() => ({
  supported: false,
  cowSupported: false,
  settings: null as MonoSubsessions | null,
  editMono: vi.fn(),
}));
vi.mock("expo-haptics", () => ({ selectionAsync: vi.fn() }));
vi.mock("expo-router", () => ({
  Stack: { Screen: () => null },
  useLocalSearchParams: () => ({ monoId: "mono" }),
}));
vi.mock("react-native", () => ({
  ActivityIndicator: () => null,
  Alert: { alert: vi.fn() },
  StyleSheet: { hairlineWidth: 1 },
  Text: ({ children }: { children: ReactNode }) => <span>{children}</span>,
  View: ({ children }: { children: ReactNode }) => <div>{children}</div>,
}));
vi.mock("@/features/connection/ConnectionProvider", () => ({
  useConnection: () => ({ connected: true, catalog: { subsessionIsolation: desktop.supported, cowIsolation: desktop.cowSupported } }),
}));
vi.mock("@/features/workspace/WorkspaceProvider", () => ({
  useWorkspace: () => ({
    monos: [{ id: "mono", name: "Helper", subsessions: desktop.settings }],
    editMono: desktop.editMono,
  }),
}));
vi.mock("@/features/models/useMonoSubsessions", () => ({
  isModel: () => false,
  useMonoSubsessions: () => ({
    catalog: { harnesses: [{ id: "codex", label: "Codex" }], models: [{ id: "new-model", name: "New model", harness: "codex" }] },
    inherited: !desktop.settings?.harness,
    harness: desktop.settings?.harness,
    model: { id: "old-model" },
    effort: { label: "Effort", options: [{ value: "high", label: "High" }] },
    effortValue: desktop.settings?.effort,
    unsetEffort: "Model default",
  }),
}));
vi.mock("@/features/sessions/ProviderLogo", () => ({ ProviderLogo: () => null }));
vi.mock("@/features/settings/SettingsUI", () => ({
  SettingsPage: ({ children }: { children: ReactNode }) => <main>{children}</main>,
  SettingsRow: ({ label, onPress }: { label: string; onPress: () => void }) => <button onClick={onPress}>{label}</button>,
}));
vi.mock("@/ui/Icon", () => ({ Icon: () => null }));
vi.mock("@/ui/theme", () => ({
  useColors: () => ({}),
  themedStyles: (fn: (colors: object) => object) => () => fn({}),
  radii: {},
}));

let container: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
const render = () => act(() => root.render(<MonoSubsessionsScreen />));
const click = (label: string) => act(() => [...container.querySelectorAll("button")].find(item => item.textContent === label)!.click());
beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  desktop.supported = false;
  desktop.cowSupported = false;
  desktop.settings = null;
  desktop.editMono.mockReset().mockResolvedValue(undefined);
  container = document.createElement("div");
  root = createRoot(container);
});
afterEach(() => act(() => root.unmount()));

it("offers isolation defaults only when the desktop supports them and sends copy-on-write", () => {
  render();
  expect(container.textContent).not.toContain("Work isolation");
  desktop.supported = true;
  desktop.cowSupported = true;
  render();
  click("New copy-on-write");
  expect(desktop.editMono).toHaveBeenLastCalledWith(expect.anything(), { subsessions: { workspaceMode: "cow" } });
});

it("keeps isolation when selecting or resetting a model and changing effort", () => {
  desktop.supported = true;
  desktop.settings = { harness: "claude", model: "old-model", workspaceMode: "cow" };
  render();
  click("New model");
  expect(desktop.editMono).toHaveBeenLastCalledWith(expect.anything(), { subsessions: { harness: "codex", model: "new-model", workspaceMode: "cow" } });
  click("Same as Helper");
  expect(desktop.editMono).toHaveBeenLastCalledWith(expect.anything(), { subsessions: { workspaceMode: "cow", effort: undefined } });
  desktop.settings = { workspaceMode: "cow", effort: "high" };
  render();
  click("Model default");
  expect(desktop.editMono).toHaveBeenLastCalledWith(expect.anything(), { subsessions: { workspaceMode: "cow", effort: undefined } });
});

it("clears isolation independently of explicit model and effort defaults", () => {
  desktop.supported = true;
  desktop.settings = { harness: "codex", model: "old-model", effort: "high", workspaceMode: "cow" };
  render();
  click("Desktop default");
  expect(desktop.editMono).toHaveBeenLastCalledWith(expect.anything(), { subsessions: { harness: "codex", model: "old-model", effort: "high", workspaceMode: undefined } });
});


it("keeps checkout and worktree choices but hides copy-on-write on an unsupported desktop platform", () => {
  desktop.supported = true;
  render();
  expect(container.textContent).toContain("Current checkout");
  expect(container.textContent).toContain("New worktree");
  expect([...container.querySelectorAll("button")].some(item => item.textContent === "New copy-on-write")).toBe(false);
  click("New worktree");
  expect(desktop.editMono).toHaveBeenLastCalledWith(expect.anything(), { subsessions: { workspaceMode: "worktree" } });
});
