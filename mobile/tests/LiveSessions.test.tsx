import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { LiveSessions } from "@/features/sessions/LiveSessions";
import { WORKING_PRESENCE_MS } from "@/features/sessions/useFinishedHandoff";
import type { FinishedSession } from "@/features/sessions/activityStore";
import type { SessionSummary } from "@/features/workspace/types";

const activity = vi.hoisted(() => ({
  working: [] as SessionSummary[],
  finished: [] as FinishedSession[],
  arrivals: {} as Record<string, number>,
  hidden: false,
  activate: (_index: number) => {},
}));

vi.mock("react-native", () => ({
  View: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  Text: ({ children }: { children?: ReactNode }) => <span>{children}</span>,
  Pressable: ({
    children,
    accessibilityLabel,
    onPress,
  }: {
    children?: ReactNode;
    accessibilityLabel?: string;
    onPress?: () => void;
  }) => (
    <button aria-label={accessibilityLabel} onClick={onPress}>
      {children}
    </button>
  ),
  Alert: { alert: vi.fn() },
}));
vi.mock("expo-haptics", () => ({
  notificationAsync: vi.fn(),
  impactAsync: vi.fn(),
  selectionAsync: vi.fn(),
  NotificationFeedbackType: { Success: "success" },
  ImpactFeedbackStyle: { Medium: "medium" },
}));
vi.mock("react-native-keyboard-controller", () => ({
  useKeyboardState: (selector: (state: { isVisible: boolean }) => boolean) =>
    selector({ isVisible: activity.hidden }),
}));
vi.mock("react-native-reanimated", async () => {
  const { useState } = await import("react");
  const resolveStyle = (style: unknown): Record<string, unknown> => {
    if (Array.isArray(style))
      return Object.assign({}, ...style.map(resolveStyle));
    if (style && typeof style === "object") {
      if ("read" in style && typeof style.read === "function")
        return style.read();
      return style as Record<string, unknown>;
    }
    return {};
  };
  const View = ({ children, style }: { children?: ReactNode; style?: unknown }) => (
    <div data-animated-height={resolveStyle(style).height}>{children}</div>
  );
  const Text = ({ children }: { children?: ReactNode }) => (
    <span>{children}</span>
  );
  const animation = { duration: () => animation, delay: () => animation };
  return {
    default: { View, Text },
    Easing: {
      in: () => {},
      out: () => {},
      inOut: () => {},
      cubic: () => {},
      sin: () => {},
    },
    FadeIn: animation,
    FadeOut: animation,
    FadeInDown: animation,
    FadeOutUp: animation,
    FlipInEasyY: animation,
    FlipOutEasyY: animation,
    useAnimatedStyle: (read: () => Record<string, unknown>) => ({ read }),
    useReducedMotion: () => true,
    useSharedValue: (initial: number) =>
      useState(() => {
        let value = initial;
        let timing:
          | { from: number; target: number; start: number; duration: number }
          | undefined;
        const get = () => {
          if (!timing) return value;
          const progress = Math.min(
            1,
            (Date.now() - timing.start) / timing.duration,
          );
          return timing.from + (timing.target - timing.from) * progress;
        };
        return {
          get,
          set: (next: number | { target: number; duration: number }) => {
            if (typeof next === "number") {
              value = next;
              timing = undefined;
            } else {
              timing = { from: get(), ...next, start: Date.now() };
            }
          },
        };
      })[0],
    withSpring: (value: number) => value,
    withTiming: (target: number, { duration }: { duration: number }) => ({
      target,
      duration,
    }),
    withSequence: (value: number) => value,
    withRepeat: (value: number) => value,
    withDelay: (_delay: number, value: number) => value,
    interpolate: () => 0,
    cancelAnimation: () => {},
  };
});
vi.mock("react-native-svg", () => ({ default: () => null, Path: () => null }));
vi.mock("@/ui/SceneFade", () => ({ GLASS_MIN_OPACITY: 0.03 }));
vi.mock("@/ui/theme", () => {
  const colors = {
    content: "white",
    muted: "gray",
    faint: "gray",
    accent: "blue",
    fill: "gray",
    background: "black",
  };
  return {
    useColors: () => colors,
    withAlpha: (color: string) => color,
    radii: { pill: 999 },
    themedStyles: (create: (value: typeof colors) => object) => () =>
      create(colors),
  };
});
vi.mock("@/ui/LiquidView", async () => {
  const { createContext, useState, useEffect } = await import("react");
  return {
    GlassPresence: createContext(null),
    LiquidView: ({ children }: { children?: ReactNode }) => (
      <div>{children}</div>
    ),
    usePopStyle: () => ({}),
    useGlassEntrance: (shown: boolean, duration: number) => {
      const [mounted, setMounted] = useState(shown);
      useEffect(() => {
        if (shown) {
          setMounted(true);
          return;
        }
        const timer = setTimeout(() => setMounted(false), duration);
        return () => clearTimeout(timer);
      }, [shown, duration]);
      return {
        mounted,
        visible: shown,
        presence: { shown, duration: duration / 1000 },
      };
    },
  };
});
vi.mock("@/ui/Ticker", () => ({
  Ticker: ({
    items,
    onActiveChange,
  }: {
    items: { key: string; text: string }[];
    onActiveChange: (index: number) => void;
  }) => {
    activity.activate = onActiveChange;
    return (
      <div data-ticker>
        {items.map((item) => (
          <span key={item.key}>{item.text}</span>
        ))}
      </div>
    );
  },
}));
vi.mock("@/features/sessions/ProviderLogo", () => ({
  ProviderLogo: () => null,
}));
vi.mock("@/features/sessions/FinishedSessions", () => ({
  FinishedSessions: ({ entries }: { entries: FinishedSession[] }) => (
    <div data-finished>
      {entries.map((entry) => (
        <span key={entry.session.id}>{entry.session.title}</span>
      ))}
    </div>
  ),
}));
vi.mock("@/features/sessions/activityStore", () => ({
  useSessionActivity: () => activity,
  markStoppedHere: vi.fn(),
}));
vi.mock("@/features/workspace/WorkspaceProvider", () => ({
  useWorkspace: () => ({
    sessions: activity.working,
    stop: vi.fn(),
    rememberSession: vi.fn(),
  }),
}));

(
  globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }
).IS_REACT_ACT_ENVIRONMENT = true;

const session = (id: string): SessionSummary => ({
  id,
  title: id,
  projectId: "project",
  harness: "claude",
  model: "Opus",
  branch: "main",
  updatedAt: 0,
  busy: true,
});
const completed = (session: SessionSummary): FinishedSession => ({
  session: { ...session, busy: false },
  at: Date.now(),
});

describe("working-to-finished pill handoff", () => {
  let root: ReturnType<typeof createRoot>;
  let container: HTMLDivElement;
  const render = () =>
    act(() =>
      root.render(
        <LiveSessions projectId="project" inView onOpen={() => {}} />,
      ),
    );
  const advance = (ms: number) => act(() => vi.advanceTimersByTime(ms));
  beforeEach(() => {
    vi.useFakeTimers();
    activity.working = [session("Build the app")];
    activity.finished = [];
    activity.arrivals = { "Build the app": 1 };
    activity.hidden = false;
    container = document.createElement("div");
    root = createRoot(container);
  });
  afterEach(() => {
    act(() => root.unmount());
    vi.useRealTimers();
  });

  it("keeps the outgoing title and subtitle, then shows the finished card after it clears", () => {
    render();
    const done = completed(activity.working[0]);
    activity.working = [];
    activity.finished = [done];
    activity.arrivals = {};
    render();
    expect(container.querySelector("[data-ticker]")?.textContent).toBe(
      "Build the app",
    );
    expect(container.textContent).toContain("From desktop");
    expect(container.querySelector("[data-finished]")?.textContent).toBe("");
    advance(WORKING_PRESENCE_MS - 1);
    expect(container.querySelector("[data-ticker]")).not.toBeNull();
    expect(container.querySelector("[data-finished]")?.textContent).toBe("");
    advance(1);
    expect(container.querySelector("[data-ticker]")).toBeNull();
    expect(container.querySelector("[data-finished]")?.textContent).toBe(
      "Build the app",
    );
  });

  it("holds the last selected session and controls when several sessions finish together", () => {
    activity.working = [session("First"), session("Second")];
    activity.arrivals = {};
    render();
    act(() => activity.activate(1));
    activity.finished = activity.working.map(completed);
    activity.working = [];
    render();
    expect(container.querySelector("[data-ticker]")?.textContent).toBe(
      "FirstSecond",
    );
    expect(
      container.querySelector('[aria-label="Next working session"]'),
    ).not.toBeNull();
    expect(container.textContent).toContain("2 of 2");
    advance(WORKING_PRESENCE_MS);
    expect(container.querySelector("[data-ticker]")).toBeNull();
    expect(container.querySelector("[data-finished]")?.textContent).toBe(
      "FirstSecond",
    );
  });

  it("collapses the outgoing pill's space throughout the exit so earlier finished pills follow", () => {
    const earlier = completed(session("Earlier result"));
    activity.finished = [earlier];
    render();
    const occupiedHeight = () =>
      Number(
        container
          .querySelector("[data-animated-height]")
          ?.getAttribute("data-animated-height"),
      );
    const initialHeight = occupiedHeight();
    expect(initialHeight).toBeGreaterThan(0);
    activity.finished = [completed(activity.working[0]), earlier];
    activity.working = [];
    render();
    expect(occupiedHeight()).toBe(initialHeight);
    advance(WORKING_PRESENCE_MS / 2);
    render();
    expect(occupiedHeight()).toBeGreaterThan(0);
    expect(occupiedHeight()).toBeLessThan(initialHeight);
    expect(container.querySelector("[data-finished]")?.textContent).toBe(
      "Earlier result",
    );
    expect(container.querySelector("[data-ticker]")).not.toBeNull();
    advance(WORKING_PRESENCE_MS / 2);
    render();
    expect(occupiedHeight()).toBe(0);
    expect(container.querySelector("[data-ticker]")).toBeNull();
    expect(container.querySelector("[data-finished]")?.textContent).toBe(
      "Build the appEarlier result",
    );
  });

  it("does not defer completed cards while another session is still running", () => {
    activity.working = [session("First"), session("Second")];
    render();
    activity.finished = [completed(activity.working[0])];
    activity.working = [activity.working[1]];
    render();
    expect(container.querySelector("[data-finished]")?.textContent).toBe(
      "First",
    );
    expect(container.querySelector("[data-ticker]")?.textContent).toBe(
      "Second",
    );
  });

  it("cancels the waiting finished card if its session resumes during the exit", () => {
    render();
    const original = activity.working[0];
    activity.finished = [completed(original)];
    activity.working = [];
    render();
    advance(100);
    activity.working = [original];
    activity.finished = [];
    render();
    advance(WORKING_PRESENCE_MS * 2);
    expect(container.querySelector("[data-finished]")?.textContent).toBe("");
    expect(container.querySelector("[data-ticker]")?.textContent).toBe(
      original.title,
    );
  });

  it("keeps the exit deadline across catalog updates and discards a card that resumes", () => {
    render();
    const original = activity.working[0];
    activity.finished = [completed(original)];
    activity.working = [];
    render();
    advance(150);
    activity.finished = [...activity.finished];
    render();
    advance(WORKING_PRESENCE_MS - 150);
    expect(container.querySelector("[data-finished]")?.textContent).toBe(
      original.title,
    );
    activity.working = [original];
    activity.finished = [];
    render();
    advance(WORKING_PRESENCE_MS * 2);
    expect(container.querySelector("[data-finished]")?.textContent).toBe("");
    expect(container.querySelector("[data-ticker]")?.textContent).toBe(
      original.title,
    );
  });
});
