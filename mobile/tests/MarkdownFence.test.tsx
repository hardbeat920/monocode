import { act, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import { Markdown } from "@/features/sessions/Markdown";

const clipboard = vi.hoisted(() => ({ setStringAsync: vi.fn() }));
vi.mock("expo-clipboard", () => clipboard);
vi.mock("@/ui/Icon", () => ({ Icon: ({ name }: { name: string }) => <span>{name}</span> }));
vi.mock("@/features/files/FileTypeIcon", () => ({
  FileTypeIcon: ({ name }: { name: string }) => <span data-file-icon={name} />,
}));
vi.mock("@/ui/theme", () => ({
  themedStyles: () => () => ({}),
  useColors: () => ({ content: "#fff", muted: "#aaa" }),
  withAlpha: (color: string) => color,
}));
vi.mock("react-native", () => ({
  Platform: { select: () => "monospace" },
  StyleSheet: { hairlineWidth: 1 },
  Linking: { openURL: vi.fn() },
  View: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  ScrollView: ({ children }: { children?: ReactNode }) => <div>{children}</div>,
  Text: ({ children }: { children?: ReactNode }) => <span>{children}</span>,
  Pressable: ({ children, onPress, accessibilityLabel }: {
    children?: ReactNode; onPress: () => void; accessibilityLabel: string;
  }) => <button onClick={onPress} aria-label={accessibilityLabel}>{children}</button>,
}));

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
let container: HTMLDivElement;
let root: ReturnType<typeof createRoot>;
const render = (text: string) => act(() => root.render(<Markdown text={text} />));
const buttons = () => Array.from(container.querySelectorAll("button"));
const copy = async (index = 0) => act(async () => { buttons()[index].click(); });

beforeEach(() => {
  vi.useFakeTimers();
  clipboard.setStringAsync.mockReset().mockResolvedValue(true);
  container = document.createElement("div");
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  vi.useRealTimers();
});

it("copies only the selected fence body and confirms success temporarily", async () => {
  render("Prose\n\n```md\n# Heading\n\n  **literal** 👋\n```\n\n~~~sh\necho hi\n~~~");
  expect(buttons()).toHaveLength(2);
  await copy();
  expect(clipboard.setStringAsync).toHaveBeenLastCalledWith("# Heading\n\n  **literal** 👋");
  expect(buttons()[0].getAttribute("aria-label")).toBe("Copied");
  expect(buttons()[0].textContent).toBe("check");
  act(() => vi.advanceTimersByTime(2000));
  expect(buttons()[0].getAttribute("aria-label")).toBe("Copy code");
  await copy(1);
  expect(clipboard.setStringAsync).toHaveBeenLastCalledWith("echo hi");
});

it.each([
  ["md", "code.md"], ["markdown", "code.md"], ["typescript", "code.ts"],
  ["python", "code.py"], ["bash", "code.sh"], ["c++", "code.cpp"],
  ["custom", "code.custom"], ["", "code.text"],
])("shows the language and matching file icon for %s", (language, fileName) => {
  render(`\`\`\`${language}\nbody\n\`\`\``);
  expect(container.querySelector("[data-file-icon]")?.getAttribute("data-file-icon")).toBe(fileName);
  expect(container.textContent).toContain(language || "text");
  expect(buttons()[0].getAttribute("aria-label")).toBe("Copy code");
});

it("copies the latest contents of an unfinished fence", async () => {
  render("```\nfirst");
  await copy();
  expect(clipboard.setStringAsync).toHaveBeenLastCalledWith("first");
  render("```\nfirst\nsecond");
  await copy();
  expect(clipboard.setStringAsync).toHaveBeenLastCalledWith("first\nsecond");
});

it("keeps the action available after a clipboard failure", async () => {
  render("```\ntext\n```");
  clipboard.setStringAsync.mockRejectedValueOnce(new Error("Unavailable"));
  await copy();
  expect(buttons()[0].getAttribute("aria-label")).toBe("Copy code");
  await copy();
  expect(buttons()[0].getAttribute("aria-label")).toBe("Copied");
});

it("clears confirmation when the fence body changes", async () => {
  render("```\nfirst");
  await copy();
  expect(buttons()[0].getAttribute("aria-label")).toBe("Copied");
  render("```\nfirst\nsecond");
  expect(buttons()[0].getAttribute("aria-label")).toBe("Copy code");
  expect(vi.getTimerCount()).toBe(0);
});

it("ignores a clipboard completion for an earlier fence body", async () => {
  let complete!: (saved: boolean) => void;
  clipboard.setStringAsync.mockReturnValueOnce(new Promise<boolean>((resolve) => { complete = resolve; }));
  render("```\nfirst");
  await copy();
  render("```\nfirst\nsecond");
  await act(async () => { complete(true); });
  expect(buttons()[0].getAttribute("aria-label")).toBe("Copy code");
  expect(vi.getTimerCount()).toBe(0);
  await copy();
  expect(buttons()[0].getAttribute("aria-label")).toBe("Copied");
});

it("does not confirm when the clipboard resolves false", async () => {
  render("```\ntext\n```");
  clipboard.setStringAsync.mockResolvedValueOnce(false);
  await copy();
  expect(buttons()[0].getAttribute("aria-label")).toBe("Copy code");
  expect(vi.getTimerCount()).toBe(0);
  await copy();
  expect(buttons()[0].getAttribute("aria-label")).toBe("Copied");
});
