// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { AgentTranscript } from "./AgentTranscript";
import { messageFilesFromClipboard } from "../../../platform/tauri/clipboard";

let container: HTMLDivElement;
let root: Root;

it("shows the user's send time next to the message actions", () => {
  act(() =>
    root.render(
      createElement(AgentTranscript, {
        blocks: [
          {
            id: "prompt",
            role: "user",
            text: "Hello",
            startedAt: Date.UTC(2026, 8, 17, 12, 30),
          },
        ],
      }),
    ),
  );
  const time = container.querySelector("time");
  expect(time).not.toBeNull();
  expect(time!.dateTime).toBe("2026-09-17T12:30:00.000Z");
  expect(
    time!.parentElement!.querySelector('[aria-label="Copy message"]'),
  ).not.toBeNull();
});

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  // Measurements here are driven explicitly; the observer never reports.
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

it("copies the full user prompt and confirms success", async () => {
  const text = "First line\nSecond line\nThird line\nFourth line\nFifth line";
  act(() =>
    root.render(
      createElement(AgentTranscript, {
        blocks: [{ id: "prompt", role: "user", text }],
      }),
    ),
  );
  const button = container.querySelector<HTMLButtonElement>(
    'button[aria-label="Copy message"]',
  );
  expect(button).not.toBeNull();
  await act(async () => button!.click());
  expect(await navigator.clipboard.readText()).toBe(text);
  expect(button!.getAttribute("aria-label")).toBe("Copied");
});

it("lets keyboard users expand and collapse a truncated prompt", async () => {
  vi.spyOn(HTMLElement.prototype, "scrollHeight", "get").mockReturnValue(100);
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockReturnValue(20);
  act(() =>
    root.render(
      createElement(AgentTranscript, {
        blocks: [
          {
            id: "prompt",
            role: "user",
            text: "First line\nSecond line\nThird line\nFourth line\nFifth line",
          },
        ],
      }),
    ),
  );

  const button = container.querySelector<HTMLButtonElement>(
    'button[aria-expanded="false"]',
  );
  expect(button?.textContent).toBe("Show more");

  await act(async () => button!.click());
  expect(button!.getAttribute("aria-expanded")).toBe("true");
  expect(button!.textContent).toBe("Show less");
  expect(container.querySelector("pre")?.classList).not.toContain(
    "line-clamp-4",
  );
});

it("remeasures prompt corners when a pooled tab is shown at a new width", () => {
  let textWidth = 200;
  let textHeight = 20;
  vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockImplementation(
    function () {
      return this.tagName === "PRE" ? textWidth : 200;
    },
  );
  vi.spyOn(HTMLElement.prototype, "scrollHeight", "get").mockImplementation(
    function () {
      return this.tagName === "PRE" ? textHeight : 200;
    },
  );
  vi.spyOn(HTMLElement.prototype, "clientHeight", "get").mockImplementation(
    function () {
      return this.tagName === "PRE" ? textHeight : 200;
    },
  );
  const getStyle = window.getComputedStyle.bind(window);
  vi.spyOn(window, "getComputedStyle").mockImplementation((element) => {
    const style = getStyle(element);
    return element.tagName === "PRE"
      ? new Proxy(style, {
          get(target, property) {
            return property === "lineHeight"
              ? "20px"
              : Reflect.get(target, property);
          },
        })
      : style;
  });

  const blocks = [
    { id: "prompt", role: "user" as const, text: "A long pasted prompt" },
  ];
  const show = (visible: boolean) =>
    act(() => root.render(createElement(AgentTranscript, { blocks, visible })));
  const bubble = () => container.querySelector(".user-message-bubble")!;

  show(true);
  expect(bubble().classList).toContain("rounded-full");

  textWidth = 0;
  textHeight = 0;
  show(false);
  expect(bubble().classList).toContain("rounded-xl");

  textWidth = 260;
  textHeight = 20;
  show(true);
  expect(bubble().classList).toContain("rounded-full");
});

it("shows a copy error instead of success and allows retry", async () => {
  const write = vi
    .spyOn(navigator.clipboard, "write")
    .mockRejectedValue(new Error("Clipboard unavailable"));
  act(() =>
    root.render(
      createElement(AgentTranscript, {
        blocks: [
          {
            id: "p",
            role: "user",
            text: "File",
            attachments: [
              {
                id: "f",
                name: "a.txt",
                mimeType: "text/plain",
                kind: "file",
                size: 1,
                data: "YQ==",
              },
            ],
          },
        ],
      }),
    ),
  );
  const button = container.querySelector<HTMLButtonElement>(
    '[aria-label="Copy message"]',
  )!;
  await act(async () => button.click());
  expect(container.querySelector('[role="alert"]')?.textContent).toContain(
    "Clipboard unavailable",
  );
  expect(button.getAttribute("aria-label")).toBe("Copy message");
  write.mockResolvedValue();
  await act(async () => button.click());
  expect(button.getAttribute("aria-label")).toBe("Copied");
  expect(container.querySelector('[role="alert"]')).toBeNull();
});

it("copies attachments even when the user message has no text", async () => {
  let written: ClipboardItem[] = [];
  vi.spyOn(navigator.clipboard, "write").mockImplementation(async (items) => {
    written = items;
  });
  act(() =>
    root.render(
      createElement(AgentTranscript, {
        blocks: [
          {
            id: "image",
            role: "user",
            text: "",
            attachments: [
              {
                id: "a",
                name: "picture.png",
                mimeType: "image/png",
                kind: "image",
                size: 3,
                data: "YWJj",
              },
            ],
          },
        ],
      }),
    ),
  );
  const button = container.querySelector<HTMLButtonElement>(
    'button[aria-label="Copy message"]',
  );
  expect(button).not.toBeNull();
  await act(async () => button!.click());
  const html = await (await written[0].getType("text/html")).text();
  expect(messageFilesFromClipboard({ getData: () => html })?.[0].name).toBe(
    "picture.png",
  );
});

it("saves the user prompt to Notes from the action row outside the bubble", async () => {
  const onSaveNote = vi.fn();
  act(() =>
    root.render(
      createElement(AgentTranscript, {
        blocks: [
          {
            id: "prompt",
            role: "user",
            text: "Keep this prompt\nWith its formatting",
          },
        ],
        onSaveNote,
      }),
    ),
  );
  const save = container.querySelector<HTMLButtonElement>(
    'button[aria-label="Save as note"]',
  );
  expect(save).not.toBeNull();
  expect(save!.closest(".user-message-bubble")).toBeNull();
  const copy = container.querySelector('button[aria-label="Copy message"]');
  expect(copy!.closest(".user-message-bubble")).toBeNull();
  expect(save!.parentElement).toBe(copy!.parentElement);
  await act(async () => save!.click());
  expect(onSaveNote).toHaveBeenCalledWith(
    "Keep this prompt\nWith its formatting",
  );
  expect(save!.getAttribute("aria-label")).toBe("Saved to Notes");
});
