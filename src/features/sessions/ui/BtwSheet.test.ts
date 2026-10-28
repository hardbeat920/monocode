// @vitest-environment happy-dom
import { act, createElement, type FormEvent } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("./Composer", () => ({
  Composer: ({
    initialDraft,
    onDraftChange,
    onSubmit,
    busy,
    allowBusySubmit,
    onStop,
  }: {
    initialDraft?: string;
    onDraftChange?: (text: string) => void;
    onSubmit: (text: string) => boolean | void;
    busy?: boolean;
    allowBusySubmit?: boolean;
    onStop?: () => void;
  }) =>
    createElement(
      "div",
      null,
      createElement("textarea", {
        "data-btw-composer": "true",
        defaultValue: initialDraft ?? "",
        onInput: (event: FormEvent<HTMLTextAreaElement>) =>
          onDraftChange?.(event.currentTarget.value),
        onKeyDown: (event: KeyboardEvent) => {
          if (event.key === "Enter") {
            onSubmit((event.currentTarget as HTMLTextAreaElement).value);
          }
        },
      }),
      busy && allowBusySubmit === false
        ? createElement(
            "button",
            { type: "button", "data-btw-stop": "true", onClick: onStop },
            "Stop",
          )
        : null,
    ),
}));

import { BtwSheet, useBtwConversation, type BtwConversation } from "./BtwSheet";
import type { Block, BtwThread } from "../model/session";

function thread(id: string, question: string, createdAt: number): BtwThread {
  return {
    id,
    sourceEndBlockId: "a1",
    createdAt,
    updatedAt: createdAt,
    status: "ready",
    harness: "claude",
    messages: [
      { id: `${id}-q`, role: "user", text: question, createdAt },
      {
        id: `${id}-a`,
        role: "assistant",
        text: `${question} answered`,
        createdAt,
      },
    ],
  };
}

function session(threads?: BtwThread[]): Block[] {
  return [
    {
      id: "u1",
      role: "user",
      text: "first",
      durationMs: 1000,
      ...(threads ? { btwThreads: threads } : {}),
    },
    { id: "a1", role: "assistant", text: "done" },
  ];
}

type Options = Parameters<typeof useBtwConversation>[0];

describe("BTW conversation", () => {
  let container: HTMLDivElement;
  let root: Root;
  let btw: BtwConversation;

  function Harness(options: Options) {
    btw = useBtwConversation(options);
    return createElement(BtwSheet, { btw });
  }

  async function render(overrides: Partial<Options> = {}) {
    await act(async () =>
      root.render(
        createElement(Harness, {
          available: true,
          blocks: session(),
          harness: "claude",
          onSubmit: vi.fn(),
          onRetry: vi.fn(),
          ...overrides,
        }),
      ),
    );
  }

  const tabs = () => [
    ...container.querySelectorAll<HTMLButtonElement>('[role="tab"]'),
  ];

  beforeEach(() => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
    vi.unstubAllGlobals();
  });

  it("stays hidden until opened", async () => {
    await render();
    expect(container.querySelector("[data-btw-overlay]")).toBeNull();
  });

  it("opens an empty tab for a bare /btw without sending", async () => {
    const onSubmit = vi.fn();
    await render({ onSubmit });
    await act(async () => void btw.openWith(""));

    expect(btw.open).toBe(true);
    expect(tabs().map((tab) => tab.textContent)).toEqual(["New question"]);
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("carries typed text into the side composer without sending", async () => {
    const onSubmit = vi.fn();
    await render({ onSubmit });
    await act(async () => void btw.openWith("half a thought", { draft: true }));

    const composer = container.querySelector<HTMLTextAreaElement>(
      "[data-btw-composer]",
    );
    expect(composer?.value).toBe("half a thought");
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("keeps an unsent question in its own tab while switching", async () => {
    await render({ blocks: session([thread("t1", "Earlier", 1)]) });
    await act(async () => void btw.openWith("half a thought", { draft: true }));
    expect(
      container.querySelector<HTMLTextAreaElement>("[data-btw-composer]")
        ?.value,
    ).toBe("half a thought");

    await act(async () => tabs()[0].click());
    expect(
      container.querySelector<HTMLTextAreaElement>("[data-btw-composer]")
        ?.value,
    ).toBe("");

    await act(async () => tabs()[1].click());
    expect(
      container.querySelector<HTMLTextAreaElement>("[data-btw-composer]")
        ?.value,
    ).toBe("half a thought");
  });

  it("keeps a pending question visible when another draft opens", async () => {
    await render();
    await act(async () => void btw.openWith("First question"));
    await act(async () => btw.startDraft());

    expect(tabs().map((tab) => tab.textContent)).toEqual([
      "First question",
      "New question",
    ]);
    expect(tabs()[0].getAttribute("aria-selected")).toBe("false");
    expect(tabs()[1].getAttribute("aria-selected")).toBe("true");
  });

  it("closes on Escape from the side composer", async () => {
    await render();
    await act(async () => void btw.openWith(""));
    const composer = container.querySelector("[data-btw-composer]");
    await act(async () =>
      composer?.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
      ),
    );
    expect(btw.open).toBe(false);
    expect(container.querySelector("[data-btw-overlay]")).toBeNull();
  });

  it("closes on Escape when nothing is focused", async () => {
    await render();
    await act(async () => void btw.openWith(""));
    (document.activeElement as HTMLElement | null)?.blur();
    await act(async () =>
      document.body.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
      ),
    );
    expect(btw.open).toBe(false);
  });

  it("sends /btw text against the latest finished turn", async () => {
    const onSubmit = vi.fn();
    await render({ onSubmit });
    await act(async () => void btw.openWith("something here..."));

    expect(onSubmit).toHaveBeenCalledWith(
      [
        expect.objectContaining({ id: "u1" }),
        expect.objectContaining({ id: "a1" }),
      ],
      expect.any(String),
      expect.any(String),
      "something here...",
      undefined,
      {},
    );
    expect(tabs()[0].textContent).toBe("something here...");
    expect(btw.running).toBe(true);
  });

  it("refuses to open when BTW is unavailable", async () => {
    await render({ available: false });
    let opened = true;
    await act(async () => {
      opened = btw.openWith("hi");
    });
    expect(opened).toBe(false);
    expect(container.querySelector("[data-btw-overlay]")).toBeNull();
  });

  it("stays closed when availability returns after forcing the sheet closed", async () => {
    await render();
    await act(async () => void btw.openWith(""));
    expect(btw.open).toBe(true);

    await render({ available: false });
    expect(btw.open).toBe(false);
    expect(container.querySelector("[data-btw-overlay]")).toBeNull();

    await render({ available: true });
    expect(btw.open).toBe(false);
    expect(container.querySelector("[data-btw-overlay]")).toBeNull();
  });

  it("opens saved threads when there is no eligible turn for a new draft", async () => {
    const onSubmit = vi.fn();
    await render({
      blocks: session([thread("t1", "Saved question", 1)]),
      harness: "fx",
      onSubmit,
    });

    let opened = false;
    await act(async () => {
      opened = btw.openWith("");
    });

    expect(opened).toBe(true);
    expect(btw.open).toBe(true);
    expect(tabs().map((tab) => tab.textContent)).toEqual(["Saved question"]);
    expect(container.textContent).toContain("Saved question answered");
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("asks the active tab from the composer and keeps text while it runs", async () => {
    const onSubmit = vi.fn();
    await render({ onSubmit });
    await act(async () => void btw.openWith(""));

    let accepted = false;
    await act(async () => {
      accepted = btw.submit("first question");
    });
    expect(accepted).toBe(true);
    expect(onSubmit).toHaveBeenCalledTimes(1);

    await act(async () => {
      accepted = btw.submit("too soon");
    });
    expect(accepted).toBe(false);
    expect(onSubmit).toHaveBeenCalledTimes(1);
  });

  it("does not become optimistic when the app rejects a question", async () => {
    const onSubmit = vi.fn(() => false);
    await render({ onSubmit });
    await act(async () => void btw.openWith(""));

    let accepted = true;
    await act(async () => {
      accepted = btw.submit("question that was not accepted");
    });

    expect(accepted).toBe(false);
    expect(btw.running).toBe(false);
    expect(onSubmit).toHaveBeenCalledTimes(1);
  });

  it("does not open or clear a submitted command when the app rejects it", async () => {
    const onSubmit = vi.fn(() => false);
    await render({ onSubmit });

    let accepted = true;
    await act(async () => {
      accepted = btw.openWith("question that was not accepted");
    });

    expect(accepted).toBe(false);
    expect(btw.open).toBe(false);
    expect(btw.running).toBe(false);
    expect(container.querySelector("[data-btw-overlay]")).toBeNull();
  });

  it("stops a streaming answer from the side composer", async () => {
    const onStop = vi.fn();
    const running: BtwThread = {
      ...thread("t1", "Still going", 1),
      status: "running",
      messages: [
        { id: "t1-q", role: "user", text: "Still going", createdAt: 1 },
      ],
    };
    await render({ blocks: session([running]), onStop });
    await act(async () => void btw.openWith(""));
    await act(async () => tabs()[0].click());

    expect(btw.running).toBe(true);
    const stop = container.querySelector<HTMLButtonElement>("[data-btw-stop]");
    expect(stop).not.toBeNull();
    await act(async () => stop?.click());
    expect(onStop).toHaveBeenCalledWith(
      expect.arrayContaining([expect.objectContaining({ id: "u1" })]),
      "t1",
    );
  });

  it("shows one tab per side thread in the session and switches between them", async () => {
    await render({
      blocks: session([
        thread("t1", "First question", 1),
        thread("t2", "Second question", 2),
      ]),
    });
    await act(async () => void btw.openWith(""));

    expect(tabs().map((tab) => tab.textContent)).toEqual([
      "First question",
      "Second question",
      "New question",
    ]);
    expect(tabs()[2].getAttribute("aria-selected")).toBe("true");

    await act(async () => tabs()[0].click());
    expect(tabs()[0].getAttribute("aria-selected")).toBe("true");
    expect(container.textContent).toContain("First question answered");
    expect(container.textContent).not.toContain("Second question answered");
  });

  it("deletes a thread from its tab and closes when no tabs remain", async () => {
    const onDelete = vi.fn();
    await render({
      blocks: session([thread("t1", "Only question", 1)]),
      onDelete,
    });
    await act(async () => void btw.openWith(""));
    // Drop the new empty tab first, then the saved thread.
    await act(async () =>
      container
        .querySelector<HTMLButtonElement>('button[title="Discard"]')
        ?.click(),
    );
    await act(async () =>
      container
        .querySelector<HTMLButtonElement>('button[title="Delete"]')
        ?.click(),
    );

    expect(onDelete).toHaveBeenCalledWith(
      expect.arrayContaining([expect.objectContaining({ id: "u1" })]),
      "t1",
    );
    expect(btw.open).toBe(false);
  });
});
