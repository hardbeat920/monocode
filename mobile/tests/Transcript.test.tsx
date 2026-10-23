import { act, type ReactElement, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { Transcript } from "@/features/sessions/Transcript";
import type { Turn } from "@/features/workspace/types";

const scrollToEnd = vi.hoisted(() => vi.fn());
const native = vi.hoisted(() => ({
  scrollTo: vi.fn(), dismissKeyboard: vi.fn(), top: 156, bottom: 106,
  focused: true, pushedIn: true,
}));
type ListProps = {
  data: Turn[];
  renderItem: (row: { item: Turn }) => ReactNode;
  onLayout: (event: { nativeEvent: { layout: { height: number } } }) => void;
  onContentSizeChange: (width: number, height: number) => void;
  contentContainerStyle: { paddingTop?: number; paddingBottom?: number }[];
  renderScrollComponent: (props: ListProps) => ReactElement<{
    blankSpace: { get: () => number };
    onContentInsetChange: (insets: { bottom: number }) => void;
  }>;
};
let listProps: ListProps;
const rowHeight = (turn: Turn) => 100 + (turn.items.length - 1) * 40;

vi.mock("react-native", async () => {
  const { forwardRef, useImperativeHandle, useLayoutEffect } = await import("react");
  type Layout = { nativeEvent: { layout: { height: number } } };
  const View = ({ children, onLayout }: {
    children?: ReactNode;
    onLayout?: (event: Layout) => void;
  }) => {
    useLayoutEffect(() => {
      // The row's turn, under whatever providers wrap it.
      let node = children as ReactElement<{ turn?: Turn; children?: ReactNode }> | undefined;
      while (node?.props && !node.props.turn)
        node = node.props.children as typeof node;
      const turn = node?.props?.turn;
      onLayout?.({ nativeEvent: { layout: { height: turn ? rowHeight(turn) : 100 } } });
    }, [children, onLayout]);
    return <div>{children}</div>;
  };
  const FlatList = forwardRef((props: ListProps, ref) => {
    listProps = props;
    useImperativeHandle(ref, () => ({ getNativeScrollRef: () => ({ scrollToEnd, scrollTo: native.scrollTo }) }));
    useLayoutEffect(() => {
      props.onLayout({ nativeEvent: { layout: { height: 800 } } });
    }, [props.onLayout]);
    return <div>{props.data.map((item) => <div key={item.id}>{props.renderItem({ item })}</div>)}</div>;
  });
  return {
    View,
    FlatList,
    ActivityIndicator: () => null,
    Keyboard: { dismiss: native.dismissKeyboard },
    StyleSheet: { create: (styles: unknown) => styles },
    useColorScheme: () => "dark",
  };
});
vi.mock("react-native-worklets", () => ({
  scheduleOnRN: (fn: (...args: unknown[]) => void, ...args: unknown[]) => fn(...args),
}));
vi.mock("react-native-reanimated", async () => {
  const { useState } = await import("react");
  return {
    default: { View: ({ children }: { children?: ReactNode }) => <div>{children}</div> },
    Easing: { out: () => {}, cubic: () => {}, bezier: () => () => 0 },
    useAnimatedReaction: () => {},
  useAnimatedStyle: () => ({}),
  useDerivedValue: (compute: () => number) => ({ get: compute }),
    useSharedValue: (initial: number) => useState(() => {
      let value = initial;
      return { get: () => value, set: (next: number) => { value = next; } };
    })[0],
    withTiming: (value: number) => value,
  };
});
vi.mock("@/features/sessions/SessionScreen", () => ({
  useSessionLayout: () => ({ top: native.top, bottom: native.bottom }),
  useJumpToEnd: () => null,
}));
vi.mock("expo-router", () => ({ useIsFocused: () => native.focused }));
vi.mock("expo-sqlite/kv-store", () => ({ default: { getItemSync: () => null } }));
vi.mock("@/ui/usePushedIn", () => ({
  usePushStarted: () => true,
  usePushedIn: () => native.pushedIn,
}));
vi.mock("@/features/sessions/ChatScrollView", () => ({ ChatScrollView: () => null }));
vi.mock("@/features/sessions/SessionErrorSheet", () => ({
  SessionErrorSheet: ({ message, isPresented, onDismiss }: {
    message: string; isPresented: boolean; onDismiss: () => void;
  }) => isPresented ? <button data-sheet onClick={onDismiss}>{message}</button> : null,
}));
vi.mock("@/features/sessions/TranscriptTurn", () => ({
  TranscriptOpened: ({ children }: { children?: ReactNode }) => children,
  TranscriptTurn: ({ turn, onError }: {
    turn: Turn; onError: (item: Turn["items"][number]) => void;
  }) => <span>{turn.id}{turn.items.map((item) =>
    item.type === "notice" && item.error
      ? <button key={item.id} data-notice={item.id} onClick={() => onError(item)}>View details</button>
      : null,
  )}</span>,
}));

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean })
  .IS_REACT_ACT_ENVIRONMENT = true;

const userTurn = (id: string, text: string): Turn => ({
  id,
  items: [{ type: "user", id, text }],
});

describe("transcript anchoring", () => {
  let root: ReturnType<typeof createRoot>;
  let container: HTMLDivElement;
  let frames: Map<number, FrameRequestCallback>;
  let frameId: number;
  let mounted = false;
  const render = (turns: Turn[], busy = false) => {
    act(() => root.render(<Transcript turns={turns} busy={busy} />));
    if (mounted) return;
    mounted = true;
    // The turns are drawn a frame after the list mounts.
    const [draw] = [...frames.entries()];
    frames.delete(draw[0]);
    act(() => draw[1](0));
  };
  const commitNativeLayout = () => act(() => {
    const padding = listProps.contentContainerStyle.at(-1)!;
    const height = (padding.paddingTop ?? 0) + (padding.paddingBottom ?? 0) +
      listProps.data.reduce((sum, turn) => sum + rowHeight(turn), 0) +
      Math.max(0, listProps.data.length - 1) * 4;
    listProps.onContentSizeChange(400, height);
    const scroll = listProps.renderScrollComponent(listProps).props;
    scroll.onContentInsetChange({ bottom: scroll.blankSpace.get() });
  });
  const flushQueuedFrames = () => {
    act(() => {
      while (frames.size > 0) {
        const next = [...frames.values()];
        frames.clear();
        next.forEach((callback) => callback(0));
      }
    });
  };
  const flushFrames = () => {
    commitNativeLayout();
    flushQueuedFrames();
  };

  beforeEach(() => {
    mounted = false;
    frameId = 0;
    frames = new Map();
    scrollToEnd.mockClear();
    native.scrollTo.mockClear();
    native.dismissKeyboard.mockClear();
    native.top = 156;
    native.bottom = 106;
    native.focused = true;
    native.pushedIn = true;
    vi.stubGlobal("__DEV__", false);
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      frames.set(++frameId, callback);
      return frameId;
    });
    vi.stubGlobal("cancelAnimationFrame", (id: number) => frames.delete(id));
    container = document.createElement("div");
    root = createRoot(container);
  });
  afterEach(() => {
    act(() => root.unmount());
    vi.unstubAllGlobals();
  });

  it("anchors a sent message once, including its desktop confirmation", () => {
    const previous = userTurn("previous", "Earlier message");
    render([previous]);
    flushFrames();
    scrollToEnd.mockClear();

    render([previous, userTurn("pending:1", "Hello")]);
    flushFrames();
    expect(scrollToEnd).toHaveBeenCalledTimes(1);
    expect(scrollToEnd).toHaveBeenLastCalledWith({ animated: true });

    const confirmed = userTurn("confirmed", "Hello");
    render([previous, confirmed]);
    flushFrames();
    expect(scrollToEnd).toHaveBeenCalledTimes(1);

    // A separate send with identical text still needs its own anchor.
    render([previous, confirmed, userTurn("pending:2", "Hello")]);
    flushFrames();
    expect(scrollToEnd).toHaveBeenCalledTimes(2);
  });

  it("opens an idle session at its end, without the anchor's blank space", () => {
    const previous = userTurn("previous", "Earlier message");
    render([previous]);
    flushFrames();
    expect(native.scrollTo).not.toHaveBeenCalled();
    expect(scrollToEnd).toHaveBeenCalledExactlyOnceWith({ animated: false });
    const scroll = listProps.renderScrollComponent(listProps).props;
    expect(scroll.blankSpace.get()).toBe(0);
  });

  it("opens a working session anchored", () => {
    render([userTurn("previous", "Earlier message")], true);
    flushFrames();
    expect(native.scrollTo).toHaveBeenCalledExactlyOnceWith({ y: 36, animated: false });
    const scroll = listProps.renderScrollComponent(listProps).props;
    expect(scroll.blankSpace.get()).toBeGreaterThan(0);
  });

  it("combines a send and confirmation arriving before the queued scroll", () => {
    const previous = userTurn("previous", "Earlier message");
    render([previous]);
    flushFrames();
    scrollToEnd.mockClear();

    render([previous, userTurn("pending:1", "Hello")]);
    render([previous, userTurn("confirmed", "Hello")]);
    flushFrames();
    expect(scrollToEnd).toHaveBeenCalledTimes(1);
  });

  it("keeps the user anchor when an assistant-only turn arrives", () => {
    const user = userTurn("user", "Hello");
    render([user]);
    flushFrames();
    scrollToEnd.mockClear();

    render([user, { id: "reply", items: [{ type: "text", id: "reply", text: "Hi" }] }]);
    flushFrames();
    expect(scrollToEnd).not.toHaveBeenCalled();
  });

  it("combines distinct anchor changes before the queued scroll", () => {
    const previous = userTurn("previous", "Earlier message");
    render([previous]);
    flushFrames();
    scrollToEnd.mockClear();

    const first = userTurn("first", "First message");
    render([previous, first]);
    render([previous, first, userTurn("second", "Second message")]);
    flushFrames();
    expect(scrollToEnd).toHaveBeenCalledTimes(1);
  });

  it("waits for the new header and native scroll space before anchoring the first prompt", () => {
    native.top = 180;
    const pending = userTurn("pending:1", "Hello");
    render([pending]);
    flushQueuedFrames();
    expect(native.scrollTo).not.toHaveBeenCalled();

    commitNativeLayout();
    // Navigation removes the empty screen's workspace line before the queued scroll runs.
    native.top = 156;
    render([pending]);
    flushQueuedFrames();
    expect(native.scrollTo).not.toHaveBeenCalled();

    flushFrames();
    expect(native.scrollTo).toHaveBeenCalledExactlyOnceWith({ y: 36, animated: false });
  });

  it("keeps the first prompt at the exact anchor when confirmation adds the reply", () => {
    render([userTurn("pending:1", "Hello")]);
    flushFrames();
    native.scrollTo.mockClear();
    const confirmed = userTurn("confirmed", "Hello");
    confirmed.items.push({ type: "text", id: "reply", text: "Hi" });
    render([confirmed]);
    flushQueuedFrames();
    expect(native.scrollTo).not.toHaveBeenCalled();

    flushFrames();
    expect(native.scrollTo).toHaveBeenCalledExactlyOnceWith({ y: 36, animated: false });
    expect(scrollToEnd).not.toHaveBeenCalled();
  });

  const failedTurn = (id: string): Turn => ({
    id,
    items: [{ type: "notice", id: `error:${id}`, text: `Failure ${id}`, error: true }],
  });
  const sheet = () => container.querySelector<HTMLButtonElement>("[data-sheet]");

  it("opens error details on tap and keeps them dismissed through updates and history", () => {
    const failed = failedTurn("latest");
    render([failed]);
    expect(sheet()).toBeNull();
    expect(native.dismissKeyboard).not.toHaveBeenCalled();
    act(() => container.querySelector<HTMLButtonElement>('[data-notice="error:latest"]')!.click());
    expect(sheet()?.textContent).toBe("Failure latest");
    expect(native.dismissKeyboard).toHaveBeenCalledTimes(1);
    act(() => sheet()!.click());
    expect(sheet()).toBeNull();

    // A watch refresh can replace objects, and paging can add older errors.
    render([failedTurn("older"), structuredClone(failed)]);
    expect(sheet()).toBeNull();
    act(() => container.querySelector<HTMLButtonElement>('[data-notice="error:latest"]')!.click());
    expect(sheet()?.textContent).toBe("Failure latest");
    act(() => sheet()!.click());

    render([failed, failedTurn("new")]);
    expect(sheet()).toBeNull();
    act(() => container.querySelector<HTMLButtonElement>('[data-notice="error:new"]')!.click());
    expect(sheet()?.textContent).toBe("Failure new");
  });

  it("leaves historical errors closed when a later turn exists", () => {
    render([failedTurn("old"), userTurn("latest", "Try again")]);
    expect(sheet()).toBeNull();
    act(() => container.querySelector<HTMLButtonElement>('[data-notice="error:old"]')!.click());
    expect(sheet()?.textContent).toBe("Failure old");
  });

  it("keeps errors closed until tapped and hides selected details while unfocused", () => {
    const failed = failedTurn("latest");
    native.focused = false;
    render([failed], true);
    expect(sheet()).toBeNull();
    native.focused = true;
    render([failed], true);
    expect(sheet()).toBeNull();
    render([failed]);
    expect(sheet()).toBeNull();
    act(() => container.querySelector<HTMLButtonElement>('[data-notice="error:latest"]')!.click());
    expect(sheet()?.textContent).toBe("Failure latest");

    native.focused = false;
    render([failed]);
    expect(sheet()).toBeNull();
    native.focused = true;
    render([failed]);
    expect(sheet()?.textContent).toBe("Failure latest");
  });
});
