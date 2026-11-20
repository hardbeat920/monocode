import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { ProjectPages, type ProjectPagesState } from "@/features/workspace/ProjectPages";

vi.mock("react-native", () => ({
  View: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  StyleSheet: { create: (styles: object) => styles },
}));
vi.mock("react-native-reanimated", () => ({
  default: { View: ({ children }: { children?: ReactNode }) => <div>{children}</div> },
}));
vi.mock("react-native-safe-area-context", async () => {
  const { createContext, use } = await import("react");
  // The workspace pane doesn't know about its descendant native screen's bars.
  const Context = createContext({ top: 0, bottom: 12, left: 0, right: 0 });
  return {
    useSafeAreaInsets: () => use(Context),
    SafeAreaProvider: ({ children }: { children?: ReactNode }) => (
      <Context value={{ top: 0, bottom: 12, left: 0, right: 84 }}>{children}</Context>
    ),
  };
});
vi.mock("@/features/workspace/projectHeader", () => ({ PROJECT_VIEWS: ["chat", "sessions"] }));
vi.mock("@/ui/ScenePager", () => ({}));
vi.mock("@/ui/ContentPane", () => ({ useContentWidth: () => 469 }));
vi.mock("@/ui/SegmentedSwitch", () => ({ SegmentedSwitch: () => null }));

it("gives both project pages screen-local insets without remounting a draft when switching pages", () => {
  (globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  function Page({ name }: { name: string }) {
    const { left, right } = useSafeAreaInsets();
    return <section aria-label={name}><output>{`${left}:${right}`}</output><input /></section>;
  }
  const container = document.createElement("div");
  const root = createRoot(container);
  const render = (view: "chat" | "sessions") => act(() => root.render(
    <ProjectPages
      pages={{ view, slide: { transform: [{ translateX: view === "chat" ? 0 : -469 }] } } as unknown as ProjectPagesState}
      chat={<Page name="Chat and live-session pills" />}
      sessions={<Page name="Sessions" />}
    />,
  ));
  try {
    render("chat");
    expect([...container.querySelectorAll("output")].map((output) => output.textContent)).toEqual(["0:84", "0:84"]);
    const draft = container.querySelector('[aria-label="Chat and live-session pills"] input') as HTMLInputElement;
    draft.value = "Unsent draft";
    render("sessions");
    render("chat");
    expect(container.querySelector('[aria-label="Chat and live-session pills"] input')).toBe(draft);
    expect(draft.value).toBe("Unsent draft");
    expect([...container.querySelectorAll("output")].map((output) => output.textContent)).toEqual(["0:84", "0:84"]);
  } finally {
    act(() => root.unmount());
  }
});
