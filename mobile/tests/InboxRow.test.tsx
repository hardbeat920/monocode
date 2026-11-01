import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { InboxRow, snoozedLabel, whyLabel, type RowHandlers } from "@/features/inbox/InboxRow";
import type { InboxItem } from "@/features/inbox/protocol";

const NOW = new Date("2026-11-01T14:00:00").getTime();

const captured = vi.hoisted(() => ({
  accessibility: undefined as undefined | ((event: { nativeEvent: { actionName: string } }) => void),
}));

vi.mock("react-native", () => ({
  View: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  Text: ({ children }: { children?: ReactNode }) => <span>{children}</span>,
  Pressable: ({
    children,
    onPress,
    accessibilityLabel,
    onAccessibilityAction,
  }: {
    children?: ReactNode;
    onPress?: () => void;
    accessibilityLabel?: string;
    onAccessibilityAction?: typeof captured.accessibility;
  }) => {
    if (onAccessibilityAction) captured.accessibility = onAccessibilityAction;
    return (
      <button aria-label={accessibilityLabel} onClick={onPress}>
        {children}
      </button>
    );
  },
  StyleSheet: { create: (styles: unknown) => styles },
}));
vi.mock("react-native-gesture-handler/ReanimatedSwipeable", () => ({
  default: ({
    children,
    renderRightActions,
  }: {
    children?: ReactNode;
    renderRightActions: (progress: unknown, translation: unknown) => ReactNode;
  }) => {
    const value = { get: () => 0, set: () => {} };
    return (
      <div>
        {children}
        <div data-actions>{renderRightActions(value, value)}</div>
      </div>
    );
  },
}));
vi.mock("react-native-reanimated", () => ({
  default: { View: ({ children }: { children?: ReactNode }) => <div>{children}</div> },
  interpolate: () => 1,
  useAnimatedReaction: () => {},
  useAnimatedStyle: () => ({}),
  useSharedValue: (initial: unknown) => ({ get: () => initial, set: () => {} }),
}));
vi.mock("react-native-worklets", () => ({ scheduleOnRN: vi.fn() }));
vi.mock("expo-haptics", () => ({
  selectionAsync: vi.fn(),
  notificationAsync: vi.fn(),
  NotificationFeedbackType: { Success: "success" },
}));
vi.mock("@/ui/Icon", () => ({ Icon: () => null }));
vi.mock("@/ui/ChoiceMenu", () => ({
  ChoiceMenu: ({
    children,
    actions,
  }: {
    children?: ReactNode;
    actions: { label: string; onPress: () => void }[];
  }) => (
    <div>
      {children}
      {actions.map((action) => (
        <button key={action.label} aria-label={`Snooze ${action.label}`} onClick={action.onPress} />
      ))}
    </div>
  ),
}));
vi.mock("@/features/inbox/ProviderMark", () => ({ ProviderMark: () => null }));
vi.mock("@/ui/theme", () => {
  const colors = {
    content: "white",
    muted: "gray",
    faint: "gray",
    accent: "blue",
    caution: "amber",
    deletions: "red",
    additions: "green",
    surfaceRaised: "gray",
    background: "black",
  };
  return {
    useColors: () => colors,
    radii: { row: 12 },
    withAlpha: (color: string) => color,
    themedStyles: (create: (value: typeof colors) => object) => () => create(colors),
  };
});

(
  globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }
).IS_REACT_ACT_ENVIRONMENT = true;

const item: InboxItem = {
  key: "github:acme/app#12",
  source: "github",
  kind: "pr",
  number: "#12",
  title: "Stream tool output",
  repo: "acme/app",
  author: { name: "maren-k" },
  status: "open",
  updatedAt: NOW - 12 * 60_000,
  unread: true,
  attention: "reviewRequested",
  attentionLabel: "Review requested by maren-k",
  done: false,
  comments: 4,
  checks: "passing",
  additions: 312,
  deletions: 48,
  url: "https://github.com/acme/app/pull/12",
};

describe("an inbox row", () => {
  let container: HTMLDivElement;
  let root: ReturnType<typeof createRoot>;
  const handlers: RowHandlers = {
    open: vi.fn(),
    done: vi.fn(),
    undone: vi.fn(),
    snooze: vi.fn(),
    startAgent: vi.fn(),
    swipeOpened: vi.fn(),
  };
  const render = (value: InboxItem = item) =>
    act(() => root.render(<InboxRow item={value} handlers={handlers} now={NOW} />));
  const button = (label: string) => {
    const found = Array.from(container.querySelectorAll("button")).find(
      (entry) => entry.getAttribute("aria-label") === label,
    );
    expect(found, label).toBeDefined();
    return found!;
  };

  beforeEach(() => {
    vi.clearAllMocks();
    captured.accessibility = undefined;
    container = document.createElement("div");
    root = createRoot(container);
  });
  afterEach(() => act(() => root.unmount()));

  it("shows the title, why it is here, where it lives and its size", () => {
    render();
    const text = container.textContent ?? "";
    expect(text).toContain("Stream tool output");
    expect(text).toContain("Review requested by maren-k");
    expect(text).toContain("acme/app · #12");
    expect(text).toContain("12m");
    expect(text).toContain("+312");
    expect(text).toContain("−48");
    expect(text).toContain("4");
  });

  it("says unread in its label, and not once read", () => {
    render();
    expect(container.querySelector("button[aria-label^='Unread, Stream']")).not.toBeNull();
    render({ ...item, unread: false });
    expect(container.querySelector("button[aria-label^='Unread']")).toBeNull();
  });

  it("opens on a tap", () => {
    render();
    act(() => button(`Unread, Stream tool output, Review requested by maren-k, acme/app`).click());
    expect(handlers.open).toHaveBeenCalledWith(item);
  });

  it("offers Snooze, Start agent and Done behind it", () => {
    render();
    act(() => button("Done").click());
    expect(handlers.done).toHaveBeenCalledWith(item);
    act(() => button("Start agent").click());
    expect(handlers.startAgent).toHaveBeenCalledWith(item);
    act(() => button("Snooze Tomorrow, 9:00").click());
    const until = (handlers.snooze as ReturnType<typeof vi.fn>).mock.calls[0][1] as number;
    expect(new Date(until).getHours()).toBe(9);
    expect(until).toBeGreaterThan(NOW);
  });

  it("offers a way back instead of Done in the Done list", () => {
    render({ ...item, done: true });
    expect(container.querySelector("button[aria-label='Done']")).toBeNull();
    act(() => button("Back to inbox").click());
    expect(handlers.undone).toHaveBeenCalled();
  });

  it("can be marked done and started from the accessibility actions", () => {
    render();
    act(() => captured.accessibility?.({ nativeEvent: { actionName: "done" } }));
    expect(handlers.done).toHaveBeenCalledWith(item);
    act(() => captured.accessibility?.({ nativeEvent: { actionName: "agent" } }));
    expect(handlers.startAgent).toHaveBeenCalledWith(item);
  });

  it("says when a snooze ends", () => {
    render({ ...item, snoozedUntil: NOW + 3_600_000 });
    expect(container.textContent).toContain("Snoozed until");
    expect(snoozedLabel(NOW + 3_600_000, NOW)).toMatch(/3:00/);
  });

  it("falls back to the status when the desktop gave no reason", () => {
    expect(whyLabel({ ...item, attentionLabel: undefined, status: "merged" })).toBe("Merged");
    expect(whyLabel({ ...item, attentionLabel: undefined })).toBe("Open pull request");
    expect(whyLabel({ ...item, attentionLabel: undefined, kind: "issue" })).toBe("Open issue");
  });
});
