// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Node } from "../api/types";
import { ModElement, toneColor } from "./ModElement";

vi.mock("../../source-control/hooks/useGitFileStatuses", () => ({
  useGitFileStatuses: () => ({
    files: new Map([["/repo/src/a.ts", "modified"]]),
    dirs: new Map(),
  }),
}));
vi.mock("../../sessions/ui/AgentMarkdown", () => ({
  AgentMarkdown: ({ text }: { text: string }) =>
    createElement("div", { "data-markdown": true }, text),
}));

const cb = (id: string) => ({ $cb: id }) as never;

let container: HTMLDivElement;
let root: Root;
let onInvoke: ReturnType<typeof vi.fn>;

function render(node: Node) {
  act(() => {
    root.render(createElement(ModElement, { node, onInvoke, cwd: "/repo" }));
  });
}

beforeEach(() => {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
  onInvoke = vi.fn();
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
});

describe("toneColor", () => {
  it("maps tones to theme tokens and ignores anything else", () => {
    expect(toneColor("accent")).toBe("var(--color-accent)");
    expect(toneColor("success")).toBe("var(--color-diff-add-fg)");
    expect(toneColor("danger")).toBe("var(--color-diff-del-fg)");
    expect(toneColor("read")).toBe("var(--tone-read)");
    expect(toneColor("teal")).toBe("var(--tone-teal)");
    expect(toneColor("#ff0000")).toBeUndefined();
    expect(toneColor("constructor")).toBeUndefined();
    expect(toneColor(undefined)).toBeUndefined();
  });
});

describe("ModElement", () => {
  it("renders Box and Text with layout and tone styles", () => {
    render({
      type: "Box",
      props: { flexDirection: "column", gap: 2, padding: 3, border: true, borderColor: "warning" },
      children: [
        { type: "Text", props: { color: "success", bold: true, mono: true }, children: ["ok ", 3] },
        { type: "Text", props: { color: "#123456" as never }, children: ["plain"] },
        false,
        null,
      ],
    });
    const box = container.firstElementChild as HTMLElement;
    expect(box.style.flexDirection).toBe("column");
    expect(box.style.gap).toBe("8px");
    expect(box.getAttribute("style")).toContain("padding: 12px");
    expect(box.style.borderColor).toBe("var(--tone-warning)");
    const [ok, plain] = Array.from(box.querySelectorAll("span"));
    expect(ok.textContent).toBe("ok 3");
    expect(ok.style.color).toBe("var(--color-diff-add-fg)");
    expect(ok.className).toContain("font-semibold");
    expect(ok.className).toContain("font-mono");
    expect(plain.style.color).toBe("");
  });

  it("renders Badge, Dot, Icon, Spinner, Skeleton and Markdown", () => {
    render({
      type: "Box",
      props: {},
      children: [
        { type: "Badge", props: { label: "OPEN", tone: "green" } },
        { type: "Dot", props: { tone: "red" } },
        { type: "Icon", props: { name: "branch", tone: "git" } },
        { type: "Icon", props: { name: "no-such-icon" } },
        { type: "Spinner", props: {} },
        { type: "Skeleton", props: { rows: 3 } },
        { type: "Markdown", props: { text: "**hi**" } },
      ],
    });
    const badge = Array.from(container.querySelectorAll("span")).find(
      (el) => el.textContent === "OPEN",
    )!;
    expect(badge.style.color).toBe("var(--tone-green)");
    expect(container.querySelector('[style*="--tone-red"]')).not.toBeNull();
    expect(container.querySelectorAll("svg")).toHaveLength(1);
    expect(container.querySelectorAll(".animate-pulse")).toHaveLength(3);
    expect(container.querySelector("[data-markdown]")?.textContent).toBe("**hi**");
  });

  it("renders nothing for unknown element types", () => {
    render({ type: "Script", props: {}, children: ["x"] } as never);
    expect(container.innerHTML).toBe("");
  });

  it("sends button presses back as callback ids", () => {
    render({
      type: "Box",
      props: {},
      children: [
        { type: "Button", props: { label: "Refresh", onPress: cb("b1") } },
        { type: "Button", props: { icon: "refresh", title: "Reload", onPress: cb("b2") } },
        { type: "Button", props: { label: "Off", disabled: true, onPress: cb("b3") } },
      ],
    });
    const buttons = container.querySelectorAll("button");
    act(() => buttons[0].click());
    act(() => buttons[1].click());
    act(() => buttons[2].click());
    expect(buttons[1].getAttribute("aria-label")).toBe("Reload");
    expect(onInvoke.mock.calls).toEqual([
      ["b1", []],
      ["b2", []],
    ]);
  });

  it("presses a Box, but not when a button inside it was pressed", () => {
    render({
      type: "Box",
      props: { onPress: cb("box") },
      children: [{ type: "Button", props: { label: "Inner", onPress: cb("inner") } }],
    });
    act(() => container.querySelector("button")!.click());
    act(() => (container.firstElementChild as HTMLElement).click());
    expect(onInvoke.mock.calls).toEqual([
      ["inner", []],
      ["box", []],
    ]);
  });

  it("reports Input changes and submits on Enter", () => {
    render({
      type: "Input",
      props: { value: "a", placeholder: "Search", onInput: cb("in"), onSubmit: cb("sub") },
    });
    const input = container.querySelector("input")!;
    expect(input.value).toBe("a");
    act(() => {
      const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
      setter.call(input, "ab");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    act(() => {
      input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    });
    expect(onInvoke.mock.calls).toEqual([
      ["in", ["ab"]],
      ["sub", ["ab"]],
    ]);
  });

  it("renders Tree rows with git colours and routes row events", () => {
    render({
      type: "Tree",
      props: {
        active: "f",
        rows: [
          { id: "d", depth: 0, kind: "dir", label: "src", open: true },
          { id: "f", depth: 1, kind: "file", label: "a.ts", path: "/repo/src/a.ts", right: [{ text: "+2", color: "success" }] },
          { id: "i", depth: 1, kind: "item", label: "live", live: "write" },
        ],
        onPress: cb("press"),
        onDoublePress: cb("open"),
        onContextMenu: cb("menu"),
        onKey: cb("key"),
      },
    });
    const rows = container.querySelectorAll<HTMLElement>('[role="treeitem"]');
    expect(rows).toHaveLength(3);
    expect(rows[0].getAttribute("aria-expanded")).toBe("true");
    expect(rows[1].getAttribute("aria-selected")).toBe("true");
    expect(rows[1].style.paddingLeft).toBe("20px");
    expect(rows[1].querySelector(".text-amber-400")?.textContent).toBe("a.ts");
    expect(rows[1].textContent).toContain("+2");
    expect(rows[2].querySelector(".shimmer-text")?.textContent).toBe("live");

    act(() => rows[1].click());
    act(() => {
      rows[1].dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    });
    act(() => {
      rows[1].dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, shiftKey: true }));
    });
    act(() => {
      rows[1].dispatchEvent(new KeyboardEvent("keydown", { key: "c", metaKey: true, bubbles: true }));
    });
    expect(onInvoke.mock.calls).toEqual([
      ["press", ["f", { ctrl: false, shift: false }]],
      ["open", ["f"]],
      ["menu", ["f", { shift: true }]],
      ["key", ["c", { ctrl: true, shift: false }]],
    ]);
  });
});
