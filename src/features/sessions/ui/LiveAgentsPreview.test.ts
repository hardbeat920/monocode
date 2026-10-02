// @vitest-environment happy-dom
import { act, createElement, type ComponentProps } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { LiveAgent } from "../model/liveAgents";
import { LiveAgentsPreview } from "./LiveAgentsPreview";

let container: HTMLDivElement;
let root: Root;

function agent(
  id: string,
  cwd: string,
  patch: Partial<LiveAgent> = {},
): LiveAgent {
  return {
    id,
    cwd,
    title: `Agent ${id}`,
    harness: "codex",
    activity: "Working",
    startedAt: 1_000,
    needsApproval: false,
    done: false,
    ...patch,
  };
}

function render(props: ComponentProps<typeof LiveAgentsPreview>) {
  act(() => root.render(createElement(LiveAgentsPreview, props)));
}

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const stored = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => stored.get(key) ?? null,
    setItem: (key: string, value: string) => stored.set(key, value),
    removeItem: (key: string) => stored.delete(key),
    clear: () => stored.clear(),
  });
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

describe("LiveAgentsPreview", () => {
  it("keeps the incoming activity order and the global four-agent cap", () => {
    render({
      agents: [
        agent("a-1", "/repo/a"),
        agent("b-1", "/repo/b", { needsApproval: true }),
        agent("a-2", "/repo/a"),
        agent("b-2", "/repo/b"),
        agent("c-1", "/repo/c"),
      ],
      groupLabels: { "/repo/a": "Alpha", "/repo/b": "Beta" },
      groupColors: {},
      groupCustomColors: {},
      groupMascots: {},
    });

    const cards = Array.from(
      container.querySelectorAll<HTMLElement>("[data-live-agent-card]"),
    );
    expect(cards.map((card) => card.dataset.liveAgentCard)).toEqual([
      "a-1",
      "b-1",
      "a-2",
      "b-2",
    ]);
    expect(cards[0].textContent).toContain("Alpha");
    expect(cards[1].textContent).toContain("Beta");
    expect(cards[0].querySelector(".mascot-active")).not.toBeNull();
    expect(container.textContent).toContain("1 more");

    const more = Array.from(container.querySelectorAll("button")).find(
      (button) => button.textContent?.includes("1 more"),
    )!;
    act(() => more.click());
    expect(container.querySelectorAll("[data-live-agent-card]")).toHaveLength(
      5,
    );
  });

  it("selects an agent from the activity list", () => {
    const onSelect = vi.fn();
    render({
      agents: [agent("a", "/repo/a"), agent("b", "/repo/b")],
      onSelect,
      groupLabels: {},
      groupColors: {},
      groupCustomColors: {},
      groupMascots: {},
    });

    act(() =>
      container
        .querySelector<HTMLButtonElement>('[data-live-agent-card="b"]')!
        .click(),
    );
    expect(onSelect).toHaveBeenCalledExactlyOnceWith("b");
  });

  it("keeps elapsed timers out of its live region", () => {
    render({
      agents: [agent("a", "/repo/a"), agent("b", "/repo/b")],
      groupLabels: {},
      groupColors: {},
      groupCustomColors: {},
      groupMascots: {},
    });

    const liveRegion = container.querySelector('[aria-live="polite"]')!;
    expect(liveRegion.textContent).toBe("2 working agents");
    expect(liveRegion.textContent).not.toMatch(/\d+s/);
    expect(container.querySelector("[role=status]")).toBeNull();
  });

  it("shows reported turn counts and exposes cache details on hover or focus", () => {
    const agents = [
      agent("a", "/repo/a", {
        turnMetrics: { inputTokens: 0, cacheReadTokens: 1_200 },
      }),
      agent("b", "/repo/b"),
    ];
    render({
      agents,
      groupLabels: {},
      groupColors: {},
      groupCustomColors: {},
      groupMascots: {},
    });

    const card = container.querySelector<HTMLElement>(
      '[data-live-agent-card="a"]',
    )!;
    expect(card.textContent).toContain("0 input");
    expect(card.textContent).toContain("1.2K cached");
    expect(card.getAttribute("aria-label")).toContain("0 input");
    expect(card.getAttribute("aria-label")).not.toContain("output");
    expect(card.getAttribute("aria-label")).toContain("1.2K cached");
    expect(card.title).toContain("1.2K cached");
    const cacheDetails = Array.from(card.children).find((child) =>
      child.textContent?.includes("cached"),
    )!;
    expect(cacheDetails.className).toContain("group-hover:visible");
    expect(cacheDetails.className).toContain("group-focus-visible:visible");

    render({
      agents: [
        agent("a", "/repo/a", {
          turnMetrics: { inputTokens: 2_000, outputTokens: 84 },
        }),
        agents[1],
      ],
      groupLabels: {},
      groupColors: {},
      groupCustomColors: {},
      groupMascots: {},
    });
    expect(card.textContent).toContain("2K input · 84 output");
    expect(card.textContent).not.toContain("cached");

    render({
      agents: [agent("a", "/repo/a"), agents[1]],
      groupLabels: {},
      groupColors: {},
      groupCustomColors: {},
      groupMascots: {},
    });
    expect(card.textContent).not.toContain("input");
    expect(card.textContent).not.toContain("output");
  });
});
