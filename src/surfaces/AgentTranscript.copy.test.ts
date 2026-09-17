// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { AgentTranscript } from "./AgentTranscript";
import { messageFilesFromClipboard } from "../lib/clipboard";

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
