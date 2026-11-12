// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { MonoComposer } from "./MonoComposer";
import { clearComposerDraft } from "../../sessions/model/draftCache";
import { saveComposerAutocorrect } from "../../settings/model/displayPrefs";
import {
  hasBlockingProjectNavigationSurface,
  shouldHandleListNavigation,
} from "../../workspace/model/tabKeys";

const { pick } = vi.hoisted(() => ({ pick: vi.fn() }));
vi.mock("../../sessions/model/attachments", async (original) => ({
  ...(await original<typeof import("../../sessions/model/attachments")>()),
  pickAttachments: pick,
}));
vi.mock("../../sessions/hooks/useFileDrop", () => ({
  useFileDrop: () => false,
}));
// The picker itself is covered on its own; here it just hands over a pick.
vi.mock("../../../shared/ui/EmojiPicker", async () => {
  const { createElement } = await import("react");
  const { createPortal } = await import("react-dom");
  return {
    EmojiPickerPopover: ({ onPick }: { onPick: (emoji: string) => void }) =>
      createPortal(
        createElement(
          "div",
          { "data-emoji-picker": "" },
          createElement(
            "button",
            { type: "button", "data-pick": "", onClick: () => onPick("😀") },
            "pick",
          ),
        ),
        document.body,
      ),
  };
});

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

it("marks only an empty Mono composer for project navigation", async () => {
  render();
  expect(field().getAttribute("data-mono-composer-empty")).toBe("true");

  type("Draft");
  expect(field().hasAttribute("data-mono-composer-empty")).toBe(false);

  type("");
  await act(async () =>
    container
      .querySelector<HTMLButtonElement>('[aria-label="Attach files"]')!
      .click(),
  );
  expect(field().hasAttribute("data-mono-composer-empty")).toBe(false);
});

it("blocks project navigation while an attachment read is pending", async () => {
  let finishRead!: (attachments: (typeof file)[]) => void;
  pick.mockReturnValueOnce(
    new Promise<(typeof file)[]>((resolve) => {
      finishRead = resolve;
    }),
  );
  render();

  await act(async () => {
    const button = container.querySelector<HTMLButtonElement>(
      '[aria-label="Attach files"]',
    )!;
    button.focus();
    button.click();
    await Promise.resolve();
  });
  const button = container.querySelector<HTMLButtonElement>(
    '[aria-label="Attach files"]',
  )!;
  expect(field().value).toBe("");
  expect(field().hasAttribute("data-mono-composer-empty")).toBe(false);
  expect(
    container
      .querySelector(".agent-chat-composer")
      ?.getAttribute("data-mono-composer-pending"),
  ).toBe("true");
  expect(
    shouldHandleListNavigation({
      blockedTarget: false,
      emptyComposerTarget: false,
      monoComposerEmptyTarget: false,
      projectNavigation: true,
      projectNavigationSurfaceOpen: hasBlockingProjectNavigationSurface(button),
      surfaceOpen: false,
    }),
  ).toBe(false);

  await act(async () => finishRead([file]));
  expect(field().hasAttribute("data-mono-composer-empty")).toBe(false);
});

it("does not mark a hidden composer as a blocking project-navigation surface", async () => {
  let finishRead!: (attachments: (typeof file)[]) => void;
  pick.mockReturnValueOnce(
    new Promise<(typeof file)[]>((resolve) => {
      finishRead = resolve;
    }),
  );
  render();
  await act(async () => {
    container
      .querySelector<HTMLButtonElement>('[aria-label="Attach files"]')!
      .click();
    await Promise.resolve();
  });

  render({ enabled: false });
  expect(
    container
      .querySelector(".agent-chat-composer")
      ?.hasAttribute("data-mono-composer-pending"),
  ).toBe(false);
  const outside = document.createElement("button");
  document.body.append(outside);
  expect(hasBlockingProjectNavigationSurface(outside)).toBe(false);
  outside.remove();

  await act(async () => finishRead([file]));
});

it("blocks project cycling from the focused emoji toggle while its portal is open", async () => {
  render({ controls: createElement("span", null, "Model") });
  const toggle = container.querySelector<HTMLButtonElement>(
    '[aria-label="Add emoji"]',
  )!;

  await act(async () => toggle.click());
  expect(document.querySelector("[data-emoji-picker]")).not.toBeNull();
  act(() => toggle.focus());
  expect(document.activeElement).toBe(toggle);
  expect(
    shouldHandleListNavigation({
      blockedTarget: false,
      emptyComposerTarget: false,
      monoComposerEmptyTarget: false,
      projectNavigation: true,
      projectNavigationSurfaceOpen: hasBlockingProjectNavigationSurface(toggle),
      surfaceOpen: false,
    }),
  ).toBe(false);
});

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

it("follows the composer autocorrect setting", () => {
  render();
  expect(field().getAttribute("spellcheck")).toBe("true");
  expect(field().getAttribute("autocorrect")).toBe("on");

  act(() => saveComposerAutocorrect(false));
  expect(field().getAttribute("spellcheck")).toBe("false");
  expect(field().getAttribute("autocorrect")).toBe("off");

  act(() => saveComposerAutocorrect(true));
  expect(field().getAttribute("spellcheck")).toBe("true");
});

it("inserts a picked emoji where the cursor was, beside attach", async () => {
  render({ controls: createElement("span", null, "Model") });
  const field = container.querySelector("textarea")!;
  act(() => {
    const setter = Object.getOwnPropertyDescriptor(
      HTMLTextAreaElement.prototype,
      "value",
    )!.set!;
    setter.call(field, "hi there");
    field.dispatchEvent(new Event("input", { bubbles: true }));
  });
  field.setSelectionRange(2, 2);
  const button = container.querySelector<HTMLButtonElement>(
    'button[aria-label="Add emoji"]',
  )!;
  expect(button.previousElementSibling?.getAttribute("aria-label")).toBe(
    "Attach files",
  );
  // The picker loads on first open.
  await act(async () => button.click());
  await vi.waitFor(() =>
    expect(document.querySelector("[data-pick]")).not.toBeNull(),
  );
  expect(button.getAttribute("aria-expanded")).toBe("true");
  act(() => document.querySelector<HTMLButtonElement>("[data-pick]")!.click());
  expect(field.value).toBe("hi😀 there");
  expect(document.querySelector("[data-pick]")).toBeNull();

  // The floating chat keeps its inline input, without the extra row.
  render();
  expect(container.querySelector('button[aria-label="Add emoji"]')).toBeNull();
});
