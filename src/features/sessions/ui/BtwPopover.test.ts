// @vitest-environment happy-dom
import { act, createElement, useEffect, useRef, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("../../../shared/ui/Popover", () => ({
  Popover: ({ children }: { children: unknown }) => children,
}));
vi.mock("./Composer", () => ({
  Composer: ({
    focused,
    initialDraft,
    draftResetToken,
  }: {
    focused?: boolean;
    initialDraft?: string;
    draftResetToken?: number;
  }) => {
    const [draft, setDraft] = useState(initialDraft ?? "");
    const resetTokenRef = useRef(draftResetToken);
    useEffect(() => {
      if (resetTokenRef.current === draftResetToken) return;
      resetTokenRef.current = draftResetToken;
      setDraft("");
    }, [draftResetToken]);
    return createElement("textarea", {
      "data-btw-composer": "true",
      "data-focused": focused ? "true" : "false",
      readOnly: true,
      value: draft,
    });
  },
}));
type BtwSubmit = (
  threadId: string,
  messageId: string,
  text: string,
  model?: string,
  modelSettings?: Record<string, string>,
) => void;

import { BtwPopover } from "./BtwPopover";

function renderPopover(
  openRequest: { id: number; text: string },
  onSubmit: BtwSubmit,
) {
  return createElement(BtwPopover, {
    harness: "claude",
    openRequest,
    onOpenRequestHandled: vi.fn(),
    onSubmit,
    onRetry: vi.fn(),
  });
}

describe("BtwPopover command requests", () => {
  let container: HTMLDivElement;
  let root: Root;

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

  it("focuses an empty composer without sending", async () => {
    const onSubmit = vi.fn();
    await act(async () =>
      root.render(renderPopover({ id: 1, text: "" }, onSubmit)),
    );

    const composer = container.querySelector<HTMLTextAreaElement>(
      "[data-btw-composer]",
    );
    expect(composer?.value).toBe("");
    expect(composer?.dataset.focused).toBe("true");
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("sends text requests and clears the composer", async () => {
    const onSubmit = vi.fn();
    await act(async () =>
      root.render(
        renderPopover({ id: 1, text: "something here..." }, onSubmit),
      ),
    );

    const composer = container.querySelector<HTMLTextAreaElement>(
      "[data-btw-composer]",
    );
    expect(onSubmit).toHaveBeenCalledWith(
      expect.any(String),
      expect.any(String),
      "something here...",
      undefined,
      {},
    );
    expect(composer?.value).toBe("");
  });
});
