import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";

import { OriginChip } from "@/features/sessions/SessionCard";
import type { Mono } from "@/features/workspace/types";

vi.mock("react-native", () => ({
  View: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  Text: ({ children }: { children?: ReactNode }) => <span>{children}</span>,
  Pressable: ({
    children,
    onPress,
    onLongPress,
    disabled,
    accessibilityLabel,
    accessibilityRole,
    style,
  }: {
    children?: ReactNode;
    onPress?: () => void;
    onLongPress?: () => void;
    disabled?: boolean;
    accessibilityLabel?: string;
    accessibilityRole?: string;
    style?: unknown;
  }) => {
    void style;
    return (
      <button
        aria-label={accessibilityLabel}
        data-role={accessibilityRole}
        disabled={disabled}
        onClick={onPress}
        onContextMenu={onLongPress}
      >
        {children}
      </button>
    );
  },
  StyleSheet: { create: (styles: unknown) => styles },
}));
vi.mock("react-native-gesture-handler/ReanimatedSwipeable", () => ({ default: () => null }));
vi.mock("react-native-reanimated", () => ({ default: { View: () => null } }));
vi.mock("@/features/orchestration/AgentDots", () => ({ AgentDots: () => null }));
vi.mock("@/features/mascots/PixelMascot", () => ({
  PixelMascot: ({ name, color }: { name: string; color: string }) => (
    <i data-mascot={name} data-color={color} />
  ),
}));
vi.mock("@/features/sessions/ProviderLogo", () => ({ ProviderLogo: () => null }));
vi.mock("@/features/sessions/TerminalSpinner", () => ({ TerminalSpinner: () => null }));
vi.mock("@/ui/Icon", () => ({ Icon: ({ name }: { name: string }) => <i data-icon={name} /> }));
vi.mock("@/ui/SwipeAction", () => ({ ACTION_SIZE: 40, SwipeAction: () => null, SwipeActions: () => null }));
vi.mock("@/ui/theme", () => ({
  themedStyles: (create: (colors: object) => object) => () => create({}),
  useColors: () => ({ faint: "gray" }),
  withAlpha: () => "black",
  radii: { row: 12 },
}));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
const mono = {
  id: "tg",
  name: "TaskGiver",
  mascot: "robot",
  color: "#ff8800",
} as Mono;

let root: ReturnType<typeof createRoot>;
afterEach(() => act(() => root.unmount()));
function render(element: ReactNode) {
  const container = document.createElement("div");
  root = createRoot(container);
  act(() => root.render(element));
  return container;
}

describe("the chip on a session a Mono started", () => {
  it("shows the Mono's mascot in its colour and its name, and opens its chat when tapped", () => {
    const onPress = vi.fn();
    const chip = render(<OriginChip origin={{ kind: "mono", mono, openable: true }} onPress={onPress} />);
    expect(chip.textContent).toBe("via TaskGiver");
    const mascot = chip.querySelector("[data-mascot]") as HTMLElement;
    expect(mascot.dataset.mascot).toBe("robot");
    expect(mascot.dataset.color).toBe("#ff8800");
    const button = chip.querySelector("button")!;
    expect(button.getAttribute("aria-label")).toBe("Started by TaskGiver");
    expect(button.dataset.role).toBe("button");
    act(() => button.click());
    expect(onPress).toHaveBeenCalledOnce();
  });

  it("hands a long press to the Mono's menu", () => {
    const onLongPress = vi.fn();
    const chip = render(
      <OriginChip origin={{ kind: "mono", mono, openable: true }} onPress={vi.fn()} onLongPress={onLongPress} />,
    );
    act(() => {
      chip.querySelector("button")!.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true }));
    });
    expect(onLongPress).toHaveBeenCalledOnce();
  });

  it("names a Mono that is not on this phone but has no way into its chat", () => {
    const onPress = vi.fn();
    const chip = render(<OriginChip origin={{ kind: "mono", mono, openable: false }} onPress={onPress} />);
    expect(chip.textContent).toBe("via TaskGiver");
    const button = chip.querySelector("button")!;
    expect(button.dataset.role).toBe("text");
    act(() => button.click());
    expect(onPress).not.toHaveBeenCalled();
  });

  it("greys out a Mono that was removed, says so, and does nothing when tapped", () => {
    const onPress = vi.fn();
    const onLongPress = vi.fn();
    const chip = render(
      <OriginChip origin={{ kind: "removed" }} onPress={onPress} onLongPress={onLongPress} />,
    );
    expect(chip.textContent).toBe("Mono removed");
    expect(chip.querySelector("[data-mascot]")).toBeNull();
    expect(chip.querySelector("[data-icon='bot']")).not.toBeNull();
    const button = chip.querySelector("button")!;
    expect(button.getAttribute("aria-label")).toBe("Started by a Mono that was removed");
    expect(button.disabled).toBe(true);
    act(() => button.click());
    expect(onPress).not.toHaveBeenCalled();
  });
});
