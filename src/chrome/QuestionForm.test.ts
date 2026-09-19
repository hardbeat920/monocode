// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { QuestionForm } from "./QuestionForm";
import type {
  UserQuestionPrompt,
  UserQuestionReply,
} from "../lib/userQuestion";

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

function prompt(multiSelect = false): UserQuestionPrompt {
  return {
    requestId: 7,
    questions: [
      {
        id: "colour",
        prompt: "Pick a colour",
        multiSelect,
        allowCustom: false,
        options: [
          { id: "red", label: "Red" },
          { id: "green", label: "Green" },
          { id: "blue", label: "Blue" },
        ],
      },
    ],
  };
}

function renderQuestion(
  onReply: (requestId: number, reply: UserQuestionReply) => void = vi.fn(),
  multiSelect = false,
) {
  act(() =>
    root.render(
      createElement(QuestionForm, {
        prompt: prompt(multiSelect),
        onReply,
      }),
    ),
  );
  return Array.from(
    container.querySelectorAll<HTMLButtonElement>("button[aria-pressed]"),
  );
}

function keyDown(target: Element, key: string) {
  act(() => {
    target.dispatchEvent(
      new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true }),
    );
  });
}

describe("QuestionForm keyboard navigation", () => {
  it("moves the highlighted option with arrow keys and selects it with Enter", () => {
    const onReply = vi.fn();
    const options = renderQuestion(onReply);

    expect(options[0].tabIndex).toBe(0);
    expect(options[1].tabIndex).toBe(-1);
    act(() => options[0].focus());

    keyDown(options[0], "ArrowDown");
    expect(document.activeElement).toBe(options[1]);
    expect(options[1].dataset.highlighted).toBe("true");
    expect(options[1].tabIndex).toBe(0);

    keyDown(options[1], "Enter");
    expect(options[1].getAttribute("aria-pressed")).toBe("true");

    act(() =>
      container
        .querySelector<HTMLButtonElement>('button[type="submit"]')!
        .click(),
    );
    expect(onReply).toHaveBeenCalledWith(7, {
      kind: "answered",
      answers: { colour: ["green"] },
    });
  });

  it("wraps arrow navigation and supports Home and End", () => {
    const options = renderQuestion();
    act(() => options[0].focus());

    keyDown(options[0], "ArrowUp");
    expect(document.activeElement).toBe(options[2]);

    keyDown(options[2], "Home");
    expect(document.activeElement).toBe(options[0]);

    keyDown(options[0], "End");
    expect(document.activeElement).toBe(options[2]);
  });

  it("uses number keys to focus and select the matching option", () => {
    const options = renderQuestion();
    act(() => options[0].focus());

    keyDown(options[0], "3");

    expect(document.activeElement).toBe(options[2]);
    expect(options[2].getAttribute("aria-pressed")).toBe("true");
    expect(options[2].getAttribute("aria-keyshortcuts")).toBe("3");
  });

  it("toggles highlighted options for multi-select questions", () => {
    const options = renderQuestion(vi.fn(), true);
    act(() => options[0].focus());

    keyDown(options[0], "Enter");
    keyDown(options[0], "ArrowDown");
    keyDown(options[1], " ");

    expect(options[0].getAttribute("aria-pressed")).toBe("true");
    expect(options[1].getAttribute("aria-pressed")).toBe("true");

    keyDown(options[1], " ");
    expect(options[1].getAttribute("aria-pressed")).toBe("false");
  });
});
