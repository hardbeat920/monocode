// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

vi.mock("../../../platform/tauri/fs", () => ({
  gitHistory: vi.fn(),
  gitCommitFiles: vi.fn(async () => []),
  gitCommitMessage: vi.fn(async () => "Subject\n"),
  subscribeGitChanged: vi.fn(() => () => undefined),
}));
vi.mock("../../../platform/tauri/clipboard", () => ({
  copyText: vi.fn(async () => undefined),
}));

import { gitHistory, type GitHistoryCommit } from "../../../platform/tauri/fs";
import { GitHistoryGraph } from "./GitHistoryGraph";

const commits: GitHistoryCommit[] = [
  {
    sha: "a".repeat(40),
    shortSha: "aaaaaaa",
    parents: [],
    author: "Ada",
    timestamp: 1_780_000_000,
    subject: "First",
    refs: [],
    head: true,
  },
  {
    sha: "b".repeat(40),
    shortSha: "bbbbbbb",
    parents: [],
    author: "Ada",
    timestamp: 1_779_000_000,
    subject: "Second",
    refs: [],
    head: false,
  },
];

const VIEWPORT = { width: 1440, height: 900 };
/** Tallest the card gets: header, subject, body, stats, refs, footer, padding. */
const CARD = { width: 448, height: 380 };

/** Where the row sits in the viewport. Scrolling the list moves this. */
let anchorTop = 300;
/** Every observer the card installed, so a test can drive intersections. */
let observers: {
  callback: IntersectionObserverCallback;
  targets: Element[];
  root: Element | Document | null;
}[] = [];

function stubAnchorRect(row: HTMLElement) {
  row.getBoundingClientRect = () =>
    ({
      left: 0,
      top: anchorTop,
      right: 300,
      bottom: anchorTop + 22,
      width: 300,
      height: 22,
      x: 0,
      y: anchorTop,
      toJSON: () => ({}),
    }) as DOMRect;
}

function stubSize(
  el: HTMLElement,
  { width, height }: { width: number; height: number },
) {
  Object.defineProperty(el, "offsetWidth", {
    value: width,
    configurable: true,
  });
  Object.defineProperty(el, "offsetHeight", {
    value: height,
    configurable: true,
  });
}

let container: HTMLDivElement;
let root: Root;

function card(): HTMLElement | null {
  return document.querySelector<HTMLElement>('[role="dialog"]');
}

/** The fixed-position frame Popover places, which carries `top`. */
function frame(): HTMLElement {
  return card()!.parentElement as HTMLElement;
}

function cardTop(): number {
  return Number(frame().style.top.replace("px", ""));
}

function scroll(): void {
  act(() => {
    window.dispatchEvent(new Event("scroll"));
  });
}

function setIntersecting(isIntersecting: boolean): void {
  act(() => {
    for (const observer of observers) {
      for (const target of observer.targets) {
        observer.callback(
          [{ target, isIntersecting } as IntersectionObserverEntry],
          {} as IntersectionObserver,
        );
      }
    }
  });
}

async function mount(cwd: string) {
  await act(async () => {
    root.render(
      createElement(GitHistoryGraph, {
        cwd,
        enabled: true,
        expanded: true,
        onToggleExpanded: () => undefined,
        onOpenCommit: () => undefined,
      }),
    );
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

/** Open a card and give the placement code a size to work with. */
async function openCard(row: HTMLElement): Promise<void> {
  stubAnchorRect(row);
  await act(async () => {
    row.focus();
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
  stubSize(frame(), CARD);
  scroll();
}

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.mocked(gitHistory).mockResolvedValue({ head: commits[0]!.sha, commits });
  window.innerWidth = VIEWPORT.width;
  window.innerHeight = VIEWPORT.height;
  anchorTop = 300;
  observers = [];
  // happy-dom has no IntersectionObserver, so stand one in that records what
  // it was asked to watch and lets the test report visibility.
  vi.stubGlobal(
    "IntersectionObserver",
    class {
      root: Element | Document | null;
      constructor(
        private callback: IntersectionObserverCallback,
        options?: IntersectionObserverInit,
      ) {
        this.root = (options?.root as Element | Document | null) ?? null;
      }
      observe(target: Element) {
        observers.push({
          callback: this.callback,
          targets: [target],
          root: this.root,
        });
      }
      unobserve() {}
      disconnect() {}
      takeRecords() {
        return [];
      }
    },
  );
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

it("follows its row down the list exactly while the card fits", async () => {
  await mount("/repo/scroll-follow");
  const row = container.querySelector<HTMLButtonElement>("[data-history-row]")!;
  await openCard(row);

  expect(cardTop()).toBe(300);

  for (const top of [260, 120, 40]) {
    anchorTop = top;
    scroll();
    expect(cardTop()).toBe(top);
  }
});

it("stays inside the window when the row is too low to align to", async () => {
  await mount("/repo/scroll-low");
  const row = container.querySelector<HTMLButtonElement>("[data-history-row]")!;
  await openCard(row);

  // The card is 380px tall, so below this the row cannot be its top edge
  // without the card hanging off the bottom of the window.
  const limit = VIEWPORT.height - CARD.height - 8;
  anchorTop = limit - 20;
  scroll();
  expect(cardTop()).toBe(limit - 20);

  anchorTop = limit + 200;
  scroll();
  // Clamped rather than allowed to overflow: the card stays readable, it just
  // no longer lines up with its row. The row is about to leave the list, which
  // is what the next test covers.
  expect(cardTop()).toBe(limit);
  expect(cardTop() + CARD.height).toBeLessThanOrEqual(VIEWPORT.height - 8);
});

it("closes the card when its row scrolls out of the list", async () => {
  await mount("/repo/scroll-away");
  const row = container.querySelector<HTMLButtonElement>("[data-history-row]")!;
  await openCard(row);
  expect(card()).not.toBeNull();

  // The observer is scoped to the scrolling list, not the window, so a row
  // hidden behind the panel's edge counts as gone.
  expect(observers).toHaveLength(1);
  expect(observers[0]!.targets).toEqual([row]);
  expect(observers[0]!.root).toBe(
    container.querySelector("[data-history-scroll]"),
  );

  setIntersecting(false);
  expect(card()).toBeNull();
});

it("keeps the card while its row is still on screen", async () => {
  await mount("/repo/scroll-stay");
  const row = container.querySelector<HTMLButtonElement>("[data-history-row]")!;
  await openCard(row);

  setIntersecting(true);
  expect(card()).not.toBeNull();

  anchorTop = 140;
  scroll();
  expect(cardTop()).toBe(140);
  expect(card()).not.toBeNull();
});

it("watches the row only while a card is open", async () => {
  await mount("/repo/scroll-idle");
  const row = container.querySelector<HTMLButtonElement>("[data-history-row]")!;

  expect(observers).toHaveLength(0);
  expect(card()).toBeNull();

  await openCard(row);
  expect(observers).toHaveLength(1);
});
