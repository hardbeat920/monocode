// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AgentMarkdown } from "./AgentMarkdown";
import {
  cadenceFadeMs,
  nextCadence,
  PACED_REVEAL_KEY,
  revealEnd,
  WORD_FADE_MS,
} from "./wordFade";

describe("revealEnd", () => {
  it("stops at the end of the word the reveal has reached", () => {
    expect(revealEnd("Hello there friend", 0, true)).toBe(5);
    expect(revealEnd("Hello there friend", 6.2, true)).toBe(11);
    expect(revealEnd("Hello there", 5, true)).toBe(5);
  });

  it("holds a word still being streamed back until it is whole", () => {
    expect(revealEnd("Hello the", 7, true)).toBe(6);
    expect(revealEnd("Hel", 1, true)).toBe(0);
  });

  it("lets the last word out once the stream has ended", () => {
    expect(revealEnd("Hello the", 7, false)).toBe(9);
  });
});

describe("cadence fade", () => {
  it("fades over three gaps of the stream, within bounds", () => {
    expect(cadenceFadeMs(100)).toBe(300);
    expect(cadenceFadeMs(10)).toBe(120);
    expect(cadenceFadeMs(1000)).toBe(400);
  });

  it("follows the gaps between bursts without one stall taking over", () => {
    expect(nextCadence(160, 60)).toBeCloseTo(130);
    expect(nextCadence(160, 60_000)).toBeCloseTo(412);
  });
});

describe("paced streaming", () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    localStorage.setItem(PACED_REVEAL_KEY, "1");
    vi.useFakeTimers({
      toFake: [
        "setTimeout",
        "clearTimeout",
        "requestAnimationFrame",
        "cancelAnimationFrame",
        "performance",
      ],
    });
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
  });

  afterEach(() => {
    localStorage.removeItem(PACED_REVEAL_KEY);
    act(() => root.unmount());
    container.remove();
    vi.unstubAllGlobals();
    vi.useRealTimers();
  });

  const reply =
    "I will review the current diff and recent commits, then check the affected code for regressions.";

  function render(text: string, streaming: boolean) {
    act(() => root.render(createElement(AgentMarkdown, { text, streaming })));
  }

  function shown() {
    return container.textContent ?? "";
  }

  function fading() {
    return !!container.querySelector(".agent-markdown.word-fading");
  }

  function word(text: string) {
    return [...container.querySelectorAll("[data-word-fade]")].find(
      (span) => span.textContent === text,
    );
  }

  it("lets a burst out a word at a time rather than all at once", () => {
    render("", true);
    render(reply, true);
    expect(shown()).toBe("");
    expect(fading()).toBe(true);

    act(() => vi.advanceTimersByTime(100));
    const early = shown();
    expect(early.length).toBeGreaterThan(0);
    expect(early.length).toBeLessThan(reply.length);
    expect(reply.startsWith(early)).toBe(true);
    expect(early.endsWith(" ")).toBe(false);

    act(() => vi.advanceTimersByTime(2_000));
    expect(shown()).toBe(reply);
  });

  it("paces the first chunk even when the reply mounts with text", () => {
    render(reply, true);
    expect(shown()).toBe("");

    act(() => vi.advanceTimersByTime(100));
    expect(shown().length).toBeGreaterThan(0);
    expect(shown().length).toBeLessThan(reply.length);

    render(reply, false);
    act(() => vi.advanceTimersByTime(2_000));
    expect(shown()).toBe(reply);
  });

  it("keeps revealing while chunks arrive faster than animation frames", () => {
    render("", true);
    let text = "";
    for (let index = 0; index < 40; index++) {
      text += `word${index} `;
      render(text, true);
      act(() => vi.advanceTimersByTime(8));
    }
    expect(shown().length).toBeGreaterThan(0);
    expect(shown().length).toBeLessThan(text.length);
    expect(text.startsWith(shown())).toBe(true);

    render(text.trimEnd(), false);
    act(() => vi.advanceTimersByTime(2_000));
    expect(shown()).toBe(text.trimEnd());
  });

  it("adds each word as its own span and never replaces one already shown", () => {
    render("", true);
    render("I will review ", true);
    act(() => vi.advanceTimersByTime(500));
    const first = word("I");
    expect(first).toBeDefined();

    // The fade plays as a span is added, so a word that keeps its element as
    // the reply grows is a word that does not fade again.
    render("I will review the diff and **recent** commits ", true);
    act(() => vi.advanceTimersByTime(1_000));
    expect(shown()).toBe("I will review the diff and recent commits");
    expect(word("I")).toBe(first);
    expect(word("recent")?.parentElement?.dataset.streamdown).toBe("strong");

    render("I will review the diff and **recent** commits", false);
    // The last word is still fading, so the spans already on screen stay put.
    expect(word("I")).toBe(first);
    act(() => vi.advanceTimersByTime(WORD_FADE_MS));
    expect(shown()).toBe("I will review the diff and recent commits");
    expect(fading()).toBe(false);
    expect(container.querySelector("[data-word-fade]")).toBeNull();
  });

  it("holds back a word still being written until the stream pauses on it", () => {
    render("", true);
    render("Hello wor", true);
    act(() => vi.advanceTimersByTime(100));
    expect(shown()).toBe("Hello");

    act(() => vi.advanceTimersByTime(300));
    expect(shown()).toBe("Hello wor");
  });

  it("finishes a stream that ends ahead of the reveal at pace, then stops fading", () => {
    render("", true);
    render(reply, true);
    render(reply, false);
    expect(shown()).toBe("");

    act(() => vi.advanceTimersByTime(2_000));
    expect(shown()).toBe(reply);
    act(() => vi.advanceTimersByTime(WORD_FADE_MS));
    // A finished reply drops its word spans, so hiding and showing it cannot
    // replay the fade.
    expect(fading()).toBe(false);
    expect(container.querySelector("[data-word-fade]")).toBeNull();
    expect(shown()).toBe(reply);
  });

  it("shows a reply that never streamed whole, as plain text", () => {
    render(reply, false);
    expect(shown()).toBe(reply);
    expect(fading()).toBe(false);
    expect(container.querySelector("[data-word-fade]")).toBeNull();
  });

  it("leaves code and links whole", () => {
    render("", true);
    render("Run `npm test` and see [the docs](https://example.com) now ", true);
    act(() => vi.advanceTimersByTime(2_000));
    expect(container.querySelector("code [data-word-fade]")).toBeNull();
    expect(container.querySelector("a [data-word-fade]")).toBeNull();
    expect(word("now")).toBeDefined();
  });
});

describe("streamed reveal", () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    vi.useFakeTimers({
      toFake: [
        "setTimeout",
        "clearTimeout",
        "requestAnimationFrame",
        "cancelAnimationFrame",
        "performance",
      ],
    });
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
  });

  afterEach(() => {
    act(() => root.unmount());
    container.remove();
    vi.unstubAllGlobals();
    vi.useRealTimers();
  });

  function render(text: string, streaming: boolean) {
    act(() => root.render(createElement(AgentMarkdown, { text, streaming })));
  }

  function shown() {
    return container.textContent ?? "";
  }

  function fadeMs() {
    const box = container.querySelector<HTMLElement>(".agent-markdown")
      ?.parentElement;
    return Number.parseFloat(box?.style.getPropertyValue("--word-fade-ms") ?? "");
  }

  it("shows a burst as soon as it arrives, fading its words in", () => {
    render("", true);
    render("I will review the diff ", true);
    expect(shown()).toBe("I will review the diff");
    expect(container.querySelector(".agent-markdown.word-fading")).not.toBeNull();
    expect(container.querySelectorAll("[data-word-fade]")).toHaveLength(5);
  });

  it("holds back a word still being written until the stream pauses on it", () => {
    render("", true);
    render("Hello wor", true);
    expect(shown()).toBe("Hello");
    act(() => vi.advanceTimersByTime(200));
    expect(shown()).toBe("Hello wor");
    render("Hello world and mo", true);
    expect(shown()).toBe("Hello world and");
  });

  it("shortens the fade for a fast stream and lengthens it for a slow one", () => {
    render("", true);
    let text = "";
    for (let index = 0; index < 20; index++) {
      text += `word${index} `;
      render(text, true);
      act(() => vi.advanceTimersByTime(16));
    }
    expect(fadeMs()).toBe(120);

    for (let index = 0; index < 20; index++) {
      text += `slow${index} `;
      render(text, true);
      act(() => vi.advanceTimersByTime(300));
    }
    expect(fadeMs()).toBe(400);
  });

  it("stops fading once the last burst has faded in", () => {
    render("", true);
    render("Done reviewing.", true);
    render("Done reviewing.", false);
    expect(shown()).toBe("Done reviewing.");
    act(() => vi.advanceTimersByTime(400));
    expect(container.querySelector(".word-fading")).toBeNull();
    expect(container.querySelector("[data-word-fade]")).toBeNull();
  });

  it("shows output already streaming when it opens whole and still", () => {
    act(() =>
      root.render(
        createElement(AgentMarkdown, {
          text: "Half way through a wor",
          streaming: true,
          revealOnMount: false,
        }),
      ),
    );
    expect(shown()).toBe("Half way through a wor");
  });
});
