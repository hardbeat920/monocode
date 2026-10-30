import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { TerminalKeys } from "@/features/terminal/TerminalKeys";

type Handlers = {
  begin?: (event: { x: number }) => void;
  update?: (event: { translationX: number; translationY: number }) => void;
  finalize?: () => void;
};
// Each key's drag, by the label of the view it is attached to.
const gestures = vi.hoisted(() => new Map<string, Handlers>());

vi.mock("react-native", () => {
  type Props = {
    children?: ReactNode | ((state: { pressed: boolean }) => ReactNode);
    accessibilityLabel?: string;
    onPressIn?: () => void;
    onPress?: () => void;
  };
  const View = ({ children, accessibilityLabel }: Props) => (
    <div aria-label={accessibilityLabel}>{children as ReactNode}</div>
  );
  return {
    View,
    ScrollView: View,
    Text: ({ children }: Props) => <span>{children as ReactNode}</span>,
    Pressable: ({
      children,
      accessibilityLabel,
      onPressIn,
      onPress,
    }: Props) => (
      <button
        aria-label={accessibilityLabel}
        onClick={() => {
          onPressIn?.();
          onPress?.();
        }}
      >
        {children as ReactNode}
      </button>
    ),
  };
});
vi.mock("react-native-gesture-handler", async () => {
  const { Children, isValidElement } = await import("react");
  const pan = () => {
    const handlers: Handlers = {};
    const builder = {
      handlers,
      minDistance: () => builder,
      shouldCancelWhenOutside: () => builder,
      onBegin: (run: Handlers["begin"]) => ((handlers.begin = run), builder),
      onUpdate: (run: Handlers["update"]) => ((handlers.update = run), builder),
      onFinalize: (run: Handlers["finalize"]) => (
        (handlers.finalize = run),
        builder
      ),
    };
    return builder;
  };
  return {
    Gesture: { Pan: pan },
    GestureDetector: ({
      gesture,
      children,
    }: {
      gesture: { handlers: Handlers };
      children: ReactNode;
    }) => {
      const child = Children.only(children);
      if (isValidElement<{ accessibilityLabel?: string }>(child))
        gestures.set(child.props.accessibilityLabel ?? "", gesture.handlers);
      return child;
    },
  };
});
vi.mock("react-native-worklets", () => ({
  scheduleOnRN: (run: (...args: unknown[]) => void, ...args: unknown[]) =>
    run(...args),
}));
vi.mock("react-native-reanimated", async () => {
  const { useState } = await import("react");
  const View = ({ children }: { children?: ReactNode }) => (
    <div>{children}</div>
  );
  const Text = ({ children }: { children?: ReactNode }) => (
    <span>{children}</span>
  );
  return {
    default: { View, Text },
    Easing: { in: () => {}, cubic: () => {} },
    interpolate: () => 0,
    useAnimatedStyle: () => ({}),
    // Holds its value, as the arrow pad keeps where it last typed in them.
    useSharedValue: (initial: number) =>
      useState(() => {
        let value = initial;
        return { get: () => value, set: (next: number) => (value = next) };
      })[0],
    withDelay: (_delay: number, value: unknown) => value,
    withSpring: (value: unknown) => value,
    withTiming: (value: unknown) => value,
  };
});
vi.mock("expo-haptics", () => ({
  selectionAsync: vi.fn(),
  impactAsync: vi.fn(),
  ImpactFeedbackStyle: { Light: "light" },
}));
vi.mock("@/ui/Icon", () => ({ Icon: () => null }));
vi.mock("@/ui/SceneFade", () => ({ GLASS_MIN_OPACITY: 0.03 }));
vi.mock("@/ui/theme", () => {
  const colors = { content: "white", accent: "blue" };
  return {
    radii: { pill: 999 },
    useColors: () => colors,
    withAlpha: (color: string) => color,
    themedStyles: (create: (value: typeof colors) => object) => () =>
      create(colors),
  };
});
vi.mock("@/ui/LiquidView", async () => {
  const { createContext } = await import("react");
  return {
    GlassPresence: createContext(null),
    LiquidView: ({
      children,
      onPress,
      accessibilityLabel,
    }: {
      children?: ReactNode;
      onPress?: () => void;
      accessibilityLabel?: string;
    }) =>
      onPress ? (
        <button aria-label={accessibilityLabel} onClick={onPress}>
          {children}
        </button>
      ) : (
        <div>{children}</div>
      ),
    usePopStyle: () => ({}),
    // Shown at once and gone at once, for the rows ••• opens.
    useGlassEntrance: (shown: boolean) => ({
      mounted: shown,
      visible: shown,
      presence: null,
    }),
  };
});

const on = {
  ctrl: vi.fn(),
  key: vi.fn(),
  paste: vi.fn(),
  hide: vi.fn(),
  action: vi.fn(),
  pad: vi.fn(),
};
let container: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
const render = (
  actions = [
    { id: "dev", name: "Run Dev", command: "npm run dev", icon: "play" },
  ],
) =>
  act(() =>
    root.render(
      <TerminalKeys
        ctrl={false}
        onCtrl={on.ctrl}
        onKey={on.key}
        onPaste={on.paste}
        onHideKeyboard={on.hide}
        actions={actions}
        onAction={on.action}
        onPadHeld={on.pad}
        onHeight={() => {}}
      />,
    ),
  );
const press = (label: string) => {
  const button = container.querySelector<HTMLButtonElement>(
    `button[aria-label="${label}"]`,
  );
  if (!button) throw new Error(`No ${label} button`);
  act(() => button.click());
};
const drag = (label: string) => {
  const handlers = gestures.get(label);
  if (!handlers) throw new Error(`No drag on ${label}`);
  return {
    begin: (x = 4) => act(() => handlers.begin?.({ x })),
    move: (translationX: number, translationY = 0) =>
      act(() => handlers.update?.({ translationX, translationY })),
    end: () => act(() => handlers.finalize?.()),
  };
};
const typed = () => on.key.mock.calls.map(([text]) => text);

beforeEach(() => {
  (
    globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }
  ).IS_REACT_ACT_ENVIRONMENT = true;
  vi.useFakeTimers();
  for (const spy of Object.values(on)) spy.mockReset();
  gestures.clear();
  container = document.createElement("div");
  root = createRoot(container);
  render();
});
afterEach(() => {
  act(() => root.unmount());
  vi.useRealTimers();
});

it("types the bar's keys, latches ctrl and pastes", () => {
  press("Escape");
  press("Tab");
  press("Control");
  press("Paste");
  expect(typed()).toEqual(["\x1b", "\t"]);
  expect(on.ctrl).toHaveBeenCalledOnce();
  expect(on.paste).toHaveBeenCalledOnce();
});

it("types ^C and / when tapped", () => {
  const interrupt = drag("Interrupt (Control C). Hold for more");
  interrupt.begin();
  interrupt.end();
  const slash = drag("Slash. Hold for more");
  slash.begin();
  slash.end();
  expect(typed()).toEqual(["\x03", "/"]);
});

it("offers ^C's alternates when held, and types the one slid onto", () => {
  const interrupt = drag("Interrupt (Control C). Hold for more");
  interrupt.begin();
  act(() => vi.advanceTimersByTime(400));
  expect(container.textContent).toContain("^Z");
  // One alternate along: ^D.
  interrupt.move(50);
  interrupt.end();
  expect(typed()).toEqual(["\x04"]);
  expect(container.textContent).not.toContain("^Z");
});

it("types an arrow for each step slid on the pad, and holds the sheet still meanwhile", () => {
  const pad = drag("Arrow keys. Slide to move");
  pad.begin();
  expect(on.pad).toHaveBeenLastCalledWith(true);
  pad.move(8);
  expect(typed()).toEqual([]);
  pad.move(20);
  pad.move(20, -30);
  pad.move(5, -30);
  pad.end();
  expect(typed()).toEqual(["\x1b[C", "\x1b[A", "\x1b[D"]);
  expect(on.pad).toHaveBeenLastCalledWith(false);
});

it("fans out more keys from •••, and folds them away again", () => {
  expect(container.querySelector('[aria-label="Tilde"]')).toBeNull();
  press("More keys");
  press("Suspend (Control Z)");
  press("Tilde");
  press("Hide keyboard");
  expect(typed()).toEqual(["\x1a", "~"]);
  expect(on.hide).toHaveBeenCalledOnce();
  press("Fewer keys");
  expect(container.querySelector('[aria-label="Tilde"]')).toBeNull();
});

it("runs the desktop's actions from their chips", () => {
  press("Run Run Dev");
  expect(on.action).toHaveBeenCalledWith("npm run dev");
  render([]);
  expect(container.querySelector('[aria-label="Run Run Dev"]')).toBeNull();
});
