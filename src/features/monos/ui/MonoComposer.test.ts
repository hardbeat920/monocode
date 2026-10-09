// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { MonoComposer } from "./MonoComposer";
import { clearComposerDraft } from "../../sessions/model/draftCache";

const { pick } = vi.hoisted(() => ({ pick: vi.fn() }));
vi.mock("../../sessions/model/attachments", async (original) => ({
  ...(await original<typeof import("../../sessions/model/attachments")>()),
  pickAttachments: pick,
}));
vi.mock("../../sessions/hooks/useFileDrop", () => ({
  useFileDrop: () => false,
}));

let container: HTMLDivElement;
let root: Root;
const file = {
  id: "file",
  name: "note.txt",
  kind: "file" as const,
  mimeType: "text/plain",
  size: 12,
  path: "/tmp/note.txt",
};
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  clearComposerDraft("chat");
  pick.mockReset().mockResolvedValue([file]);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
  clearComposerDraft("chat");
  vi.unstubAllGlobals();
});
function render(extra: Partial<Parameters<typeof MonoComposer>[0]> = {}) {
  act(() =>
    root.render(
      createElement(MonoComposer, {
        sessionId: "chat",
        name: "Captain",
        onSubmit: () => true,
        ...extra,
      }),
    ),
  );
}
function field() {
  return container.querySelector<HTMLTextAreaElement>(
    '[aria-label="Message Captain"]',
  )!;
}
function type(text: string) {
  const input = field();
  const props = Object.entries(input).find(([key]) =>
    key.startsWith("__reactProps"),
  )![1];
  act(() => props.onChange({ target: { value: text } }));
}
function submit() {
  container
    .querySelector("form")!
    .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
}

it("consumes quotes once and adds them to the current draft", () => {
  const consumed = vi.fn();
  render();
  type("My question");
  const quoteRequest = { id: 1, text: "Selected answer" };
  render({ quoteRequest, onQuoteRequestConsumed: consumed });
  expect(field().value).toBe("My question\n\n> Selected answer\n\n");
  render({ quoteRequest, onQuoteRequestConsumed: consumed, enabled: false });
  expect(field().value.match(/Selected answer/g)).toHaveLength(1);
  expect(consumed).toHaveBeenCalledWith(1);
});

it("associates each composer with its own goal help text", () => {
  act(() =>
    root.render(
      createElement(
        "div",
        null,
        createElement(MonoComposer, {
          sessionId: "chat",
          name: "Captain",
          onSubmit: () => true,
        }),
        createElement(MonoComposer, {
          sessionId: "other-chat",
          name: "Navigator",
          onSubmit: () => true,
        }),
      ),
    ),
  );

  const fields = Array.from(
    container.querySelectorAll<HTMLTextAreaElement>(
      "textarea[aria-describedby]",
    ),
  );
  const helpIds = fields.map((input) => input.getAttribute("aria-describedby"));
  expect(new Set(helpIds).size).toBe(2);
  for (const id of helpIds) {
    expect(id).not.toBeNull();
    expect(document.getElementById(id!)?.textContent).toContain("/goal");
  }
});

it("sends consecutive messages without duplicating rapid Enter presses", () => {
  const onSubmit = vi.fn(() => true);
  render({ onSubmit });
  type("First");
  act(() => {
    submit();
    submit();
  });
  type("Second");
  act(submit);
  expect(onSubmit.mock.calls).toEqual([
    ["First", []],
    ["Second", []],
  ]);
  expect(field().value).toBe("");
});

it("retains text and attachments if the message is rejected", async () => {
  const onSubmit = vi.fn(() => false);
  render({ onSubmit });
  type("Keep these");
  await act(async () =>
    container
      .querySelector<HTMLButtonElement>('[aria-label="Attach files"]')!
      .click(),
  );
  act(submit);
  expect(onSubmit).toHaveBeenCalledWith("Keep these", [file]);
  expect(field().value).toBe("Keep these");
  expect(container.querySelector('[title="/tmp/note.txt"]')).not.toBeNull();
});

it("waits for attachment reads so they cannot leak into the next message", async () => {
  let accept!: (files: (typeof file)[]) => void;
  pick.mockReturnValue(
    new Promise((resolve) => {
      accept = resolve;
    }),
  );
  const onSubmit = vi.fn(() => true);
  render({ onSubmit });
  type("With this file");
  await act(async () =>
    container
      .querySelector<HTMLButtonElement>('[aria-label="Attach files"]')!
      .click(),
  );
  expect(
    container.querySelector<HTMLButtonElement>('[aria-label="Send"]')!.disabled,
  ).toBe(true);
  act(submit);
  expect(onSubmit).not.toHaveBeenCalled();
  await act(async () => accept([file]));
  act(submit);
  expect(onSubmit).toHaveBeenCalledExactlyOnceWith("With this file", [file]);
  expect(container.querySelector('[title="/tmp/note.txt"]')).toBeNull();
});

it("waits for delivery and keeps a rejected message available to retry", async () => {
  let acknowledge!: (accepted: boolean) => void;
  const onSubmit = vi.fn(
    () =>
      new Promise<boolean>((resolve) => {
        acknowledge = resolve;
      }),
  );
  render({ onSubmit });
  type("Keep this draft");
  act(() => {
    submit();
    submit();
  });
  expect(onSubmit).toHaveBeenCalledTimes(1);
  expect(field().value).toBe("Keep this draft");
  expect(field().disabled).toBe(true);
  await act(async () => {
    acknowledge(false);
  });
  expect(field().value).toBe("Keep this draft");
  expect(field().disabled).toBe(false);
  act(() => submit());
  await act(async () => {
    acknowledge(true);
  });
  expect(field().value).toBe("");
  expect(onSubmit).toHaveBeenCalledTimes(2);
});
