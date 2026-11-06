import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { LeadLink } from "@/features/sessions/LeadLink";
import type { SessionSummary } from "@/features/workspace/types";

const app = vi.hoisted(() => ({ dismissTo: vi.fn() }));

vi.mock("react-native", () => ({
  Text: ({ children }: { children?: ReactNode }) => <span>{children}</span>,
  Pressable: ({
    children,
    onPress,
    accessibilityLabel,
  }: {
    children?: ReactNode;
    onPress?: () => void;
    accessibilityLabel?: string;
  }) => (
    <button aria-label={accessibilityLabel} onClick={onPress}>
      {children}
    </button>
  ),
  Platform: { OS: "ios" },
  StyleSheet: { create: (styles: unknown) => styles },
  useColorScheme: () => "dark",
}));
vi.mock("expo-sqlite/kv-store", () => ({
  default: { getItemSync: () => null, setItem: async () => {} },
}));
vi.mock("expo-router", () => ({ router: { dismissTo: app.dismissTo } }));
vi.mock("@/ui/Icon", () => ({ Icon: () => null }));

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  vi.clearAllMocks();
  host = document.createElement("div");
  document.body.append(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

it("takes a subagent's chat back to its lead", async () => {
  const lead = { id: "lead", projectId: "p", title: "Session store refactor" } as SessionSummary;
  await act(async () => root.render(<LeadLink lead={lead} />));
  expect(host.textContent).toContain("Agent of Session store refactor");
  await act(async () => host.querySelector("button")!.click());
  expect(app.dismissTo).toHaveBeenCalledWith({
    pathname: "/project/[projectId]/session/[sessionId]",
    params: { projectId: "p", sessionId: "lead" },
  });
});
