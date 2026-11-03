import { act, useEffect, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { SegmentedSwitch } from "@/ui/SegmentedSwitch.android";

const animation = vi.hoisted(() => ({ reaction: undefined as undefined | (() => void) }));
vi.mock("react-native-reanimated", () => ({
  useAnimatedReaction: (read: () => number | null, react: (next: number | null, previous: number | null) => void) => {
    useEffect(() => {
      let previous: number | null = null;
      animation.reaction = () => { const next = read(); react(next, previous); previous = next; };
      animation.reaction();
    });
  },
}));
vi.mock("react-native-worklets", () => ({ scheduleOnRN: (fn: (next: number | null) => void, next: number | null) => fn(next) }));
vi.mock("expo-haptics", () => ({ selectionAsync: vi.fn() }));
vi.mock("react-native", () => ({
  View: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  Text: ({ children }: { children?: ReactNode }) => <span>{children}</span>,
  Pressable: ({ children, onPress, accessibilityLabel, accessibilityState }: {
    children?: ReactNode;
    onPress: () => void;
    accessibilityLabel: string;
    accessibilityState: { selected: boolean };
  }) => <button aria-label={accessibilityLabel} aria-selected={accessibilityState.selected} onClick={onPress}>{children}</button>,
}));
vi.mock("@/ui/LiquidView", () => ({
  LiquidView: ({ children }: { children: ReactNode }) => <div>{children}</div>,
}));
vi.mock("@/ui/theme", () => ({
  themedStyles: (create: (colors: object) => object) => () => create({ content: "white", thumb: "gray" }),
}));

it("keeps the selected label visible without layout events, across switches and remounts", () => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  const container = document.createElement("div");
  const root = createRoot(container);
  const onChange = vi.fn();
  const segments = [
    { value: "new", label: "New", icon: <i data-icon="compose" /> },
    { value: "sessions", label: "Sessions", icon: <i data-icon="list" /> },
  ];
  const render = (value: string, key = "first") => act(() => root.render(
    <SegmentedSwitch key={key} segments={segments} value={value} onChange={onChange} />,
  ));
  try {
    // No onLayout events or animated progress updates are delivered.
    render("new");
    expect(container.textContent).toBe("New");
    expect(container.querySelectorAll("[data-icon]")).toHaveLength(2);
    const buttons = container.querySelectorAll("button");
    act(() => buttons[0].click());
    expect(onChange).not.toHaveBeenCalled();
    act(() => buttons[1].click());
    expect(onChange).toHaveBeenCalledWith("sessions");
    render("sessions");
    expect(container.textContent).toBe("Sessions");
    expect(container.querySelector('[aria-selected="true"]')?.textContent).toBe("Sessions");
    render("new", "remounted");
    expect(container.textContent).toBe("New");
  } finally {
    act(() => root.unmount());
  }
});

it("updates selected labels during a swipe before the pager commits value", () => {
  const container = document.createElement("div");
  const root = createRoot(container);
  let position = 0;
  const progress = { get: () => position } as import("react-native-reanimated").SharedValue<number>;
  const segments = [
    { value: "new", label: "New", icon: <i /> },
    { value: "sessions", label: "Sessions", icon: <i /> },
  ];
  try {
    act(() => root.render(<SegmentedSwitch segments={segments} value="new" progress={progress} onChange={() => {}} />));
    expect(container.textContent).toBe("New");
    act(() => { position = 0.6; animation.reaction?.(); });
    expect(container.textContent).toBe("Sessions");
    expect(container.querySelector('[aria-selected="true"]')?.textContent).toBe("Sessions");
    act(() => { position = -2; animation.reaction?.(); });
    expect(container.textContent).toBe("New");
    act(() => root.render(<SegmentedSwitch segments={segments} value="sessions" onChange={() => {}} />));
    expect(container.textContent).toBe("Sessions");
  } finally { act(() => root.unmount()); }
});
