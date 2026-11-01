import { act, isValidElement, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// Renders of each text element, by what it shows: a paragraph or emphasis by
// its first word, a fading word by the word.
const renders = vi.hoisted(() => ({ byKey: new Map<string, number>() }));
const keyOf = (children: ReactNode): string => {
  if (typeof children === "string") return children;
  if (isValidElement(children)) {
    const nodes = (children.props as { nodes?: { text?: string; children?: { text?: string }[] }[] }).nodes;
    const first = nodes?.[0];
    return `#${first?.text ?? first?.children?.[0]?.text ?? "?"}`;
  }
  return "?";
};
vi.mock("react-native", () => ({
  Linking: {},
  Platform: { select: (o: { default: string }) => o.default },
  ScrollView: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  StyleSheet: { create: <T,>(s: T) => s, hairlineWidth: 1, absoluteFill: {} },
  Text: ({ children, selectable }: { children?: ReactNode; selectable?: boolean }) => {
    const key = keyOf(children) + (selectable ? "¶" : "");
    renders.byKey.set(key, (renders.byKey.get(key) ?? 0) + 1);
    return <span>{children}</span>;
  },
  View: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
}));
vi.mock("@/features/files/FileTypeIcon", () => ({ FileTypeIcon: () => null }));
vi.mock("@/ui/theme", () => ({
  themedStyles: () => () => new Proxy({}, { get: () => ({}) }),
  useColors: () => ({ content: "#eeeeee" }),
  withAlpha: (hex: string, alpha: number) => {
    alphaCalls.count++;
    return `${hex}@${alpha}`;
  },
}));
const alphaCalls = vi.hoisted(() => ({ count: 0 }));

import { Markdown } from "@/features/sessions/Markdown";

(
  globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }
).IS_REACT_ACT_ENVIRONMENT = true;

const count = (key: string) => renders.byKey.get(key) ?? 0;
const frame = (ms = 16) => act(async () => void (await vi.advanceTimersByTimeAsync(ms)));
let root: ReturnType<typeof createRoot>;

beforeEach(() => {
  vi.useFakeTimers({
    toFake: ["requestAnimationFrame", "cancelAnimationFrame", "performance", "setTimeout", "clearTimeout"],
  });
  renders.byKey.clear();
  alphaCalls.count = 0;
  root = createRoot(document.createElement("div"));
});
afterEach(() => {
  act(() => root.unmount());
  vi.useRealTimers();
});

const FINISHED = "Alpha one two.\n\nBeta three four.\n\n";

describe("Markdown word fade", () => {
  it("re-renders only what is mid-fade on each frame", async () => {
    // Two finished paragraphs are there when it opens, settled; then the third streams in.
    await act(async () => root.render(<Markdown text={FINISHED + "**Gamma bold** start"} />));
    await act(async () =>
      root.render(<Markdown text={FINISHED + "**Gamma bold** start now streaming words"} streaming />),
    );
    renders.byKey.clear();
    alphaCalls.count = 0;
    // Ten 60 Hz frames, all inside the fade.
    for (let i = 0; i < 10; i++) await frame();

    // Finished blocks: zero renders.
    expect(count("#Alpha one two.¶")).toBe(0);
    expect(count("#Beta three four.¶")).toBe(0);
    // The settled bold text inside the block being written: zero renders.
    expect(count("#Gamma bold")).toBe(0);
    // The block being written renders once per frame.
    expect(count("#Gamma bold¶")).toBeGreaterThanOrEqual(9);
    expect(count("#Gamma bold¶")).toBeLessThanOrEqual(11);
    // Only the words that arrived (now, streaming, words) have spans, and they
    // re-render only as their colour changes.
    // ("words", the last, is held back 150 ms until whole, so it joins late.)
    expect(count("words")).toBeGreaterThanOrEqual(1);
    for (const word of ["now", "streaming"]) {
      expect(count(word)).toBeGreaterThanOrEqual(8);
      expect(count(word)).toBeLessThanOrEqual(10);
    }
    expect(count("start")).toBe(0);
    expect(count("Alpha")).toBe(0);
  });

  it("finds each frame's colour in a cache for words that share a burst", async () => {
    await act(async () => root.render(<Markdown text="Opening line." />));
    await act(async () => root.render(<Markdown text="Opening line. one two three four five six" streaming />));
    alphaCalls.count = 0;
    for (let i = 0; i < 10; i++) await frame();
    // Six words born together share an alpha: one colour built per frame, not six.
    expect(alphaCalls.count).toBeLessThanOrEqual(11);
  });

  it("goes quiet when the fade is over", async () => {
    await act(async () => root.render(<Markdown text="Opening." />));
    await act(async () => root.render(<Markdown text="Opening. then more words" streaming />));
    for (let i = 0; i < 40; i++) await frame();
    renders.byKey.clear();
    for (let i = 0; i < 20; i++) await frame();
    expect([...renders.byKey.values()].reduce((a, b) => a + b, 0)).toBe(0);
  });
});
