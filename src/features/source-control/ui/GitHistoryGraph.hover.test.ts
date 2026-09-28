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

import {
  gitCommitMessage,
  gitHistory,
  type GitHistoryCommit,
} from "../../../platform/tauri/fs";
import { clearCommitMessageCache } from "../model/commitMessage";
import { clearCommitStatsCache } from "../model/commitStats";
import { GitHistoryGraph } from "./GitHistoryGraph";

const commits: GitHistoryCommit[] = [
  {
    sha: "a".repeat(40),
    shortSha: "aaaaaaa",
    parents: ["b".repeat(40)],
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

let container: HTMLDivElement;
let root: Root;

function historyRows(): HTMLButtonElement[] {
  return Array.from(
    container.querySelectorAll<HTMLButtonElement>("[data-history-row]"),
  );
}

function card(): HTMLElement | null {
  return document.querySelector<HTMLElement>('[role="dialog"]');
}

function cardActions(): HTMLButtonElement[] {
  return Array.from(
    card()?.querySelectorAll<HTMLButtonElement>("button") ?? [],
  );
}

function key(target: Element, init: KeyboardEventInit) {
  act(() => {
    target.dispatchEvent(
      new KeyboardEvent("keydown", {
        bubbles: true,
        cancelable: true,
        ...init,
      }),
    );
  });
}

async function mount(cwd: string, selectedSha?: string) {
  await act(async () => {
    root.render(
      createElement(GitHistoryGraph, {
        cwd,
        enabled: true,
        expanded: true,
        selectedSha,
        onToggleExpanded: () => undefined,
        onOpenCommit: () => undefined,
      }),
    );
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

/**
 * Focus a row and let the card reveal. The card waits for the commit message
 * before painting, so it is not on screen the instant the row takes focus.
 */
async function focusRow(row: HTMLElement) {
  await act(async () => {
    row.focus();
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

/** Open a card the way a pointer does, with focus left somewhere else. */
async function openCardOnHover(row: HTMLElement) {
  vi.useFakeTimers();
  act(() => {
    row.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
  });
  act(() => {
    vi.advanceTimersByTime(300);
  });
  vi.useRealTimers();
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.mocked(gitHistory).mockResolvedValue({ head: commits[0]!.sha, commits });
  vi.mocked(gitCommitMessage).mockResolvedValue("Subject\n");
  // Module singletons that outlive the test.
  clearCommitMessageCache();
  clearCommitStatsCache();
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

it("tabs into the commit actions, back to the row, and forward to the next row", async () => {
  await mount("/repo/hover-keyboard");
  const rows = historyRows();
  expect(rows).toHaveLength(2);

  await focusRow(rows[0]!);
  expect(card()).not.toBeNull();

  key(rows[0]!, { key: "Tab" });
  expect(document.activeElement).toBe(cardActions()[0]);

  key(document.activeElement!, { key: "Tab", shiftKey: true });
  expect(document.activeElement).toBe(rows[0]);

  // Shift+Tab back to the row keeps the card open: focus is still inside the
  // row's disclosure, so the card is still relevant.
  expect(card()).not.toBeNull();

  const last = cardActions().at(-1)!;
  act(() => last.focus());
  key(last, { key: "Tab" });
  expect(document.activeElement).toBe(rows[1]);
});

it("closes on Escape without reopening the card it dismissed", async () => {
  await mount("/repo/hover-escape");
  const row = historyRows()[0]!;

  // Focus is inside the card, so Escape has to hand focus back to the row.
  // Focus fires the row's onFocus, which opens the card, so this is the case
  // where a dismissal can silently undo itself.
  await focusRow(row);
  key(row, { key: "Tab" });
  expect(document.activeElement).toBe(cardActions()[0]);

  act(() =>
    document.activeElement!.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Escape",
        bubbles: true,
        cancelable: true,
      }),
    ),
  );

  expect(document.activeElement).toBe(row);
  expect(card()).toBeNull();

  // The guard is consumed, so ordinary focus still opens the card afterwards.
  act(() => row.blur());
  await focusRow(row);
  expect(card()).not.toBeNull();
});

it("holds Tab during the reveal gap instead of dropping out of the list", async () => {
  await mount("/repo/hover-reveal-gap");
  const rows = historyRows();
  const row = rows[0]!;

  // First focus of this commit, so the message is uncached and the card is
  // wanted but not painted.
  let release!: (text: string) => void;
  vi.mocked(gitCommitMessage).mockReturnValue(
    new Promise<string>((resolve) => {
      release = resolve;
    }),
  );

  act(() => row.focus());
  expect(card()).toBeNull();

  // Tab must not fall through: the next row would take focus and its blur
  // would close this card.
  const event = new KeyboardEvent("keydown", {
    key: "Tab",
    bubbles: true,
    cancelable: true,
  });
  act(() => {
    row.dispatchEvent(event);
  });
  expect(event.defaultPrevented).toBe(true);
  expect(document.activeElement).toBe(row);

  await act(async () => {
    release("First\n\nBody");
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
  expect(card()).not.toBeNull();
  expect(document.activeElement).toBe(cardActions()[0]);
  expect(rows[1]).not.toBe(document.activeElement);
});

it("leaves focus alone when Escape dismisses a card the pointer opened", async () => {
  await mount("/repo/hover-escape-pointer");
  const row = historyRows()[0]!;
  const outside = document.createElement("button");
  document.body.append(outside);
  act(() => outside.focus());

  // Never held focus, so Escape has nothing to give back.
  await openCardOnHover(row);
  expect(card()).not.toBeNull();
  expect(document.activeElement).toBe(outside);

  act(() =>
    window.dispatchEvent(
      new KeyboardEvent("keydown", {
        key: "Escape",
        bubbles: true,
        cancelable: true,
      }),
    ),
  );

  expect(card()).toBeNull();
  expect(document.activeElement).toBe(outside);
  outside.remove();
});

it("closes when focus leaves the card for somewhere outside", async () => {
  await mount("/repo/hover-focus-leave");
  const row = historyRows()[0]!;
  const outside = document.createElement("button");
  document.body.append(outside);

  await focusRow(row);
  key(row, { key: "Tab" });
  expect(card()).not.toBeNull();

  // React's onBlur listens for focusout, which is the bubbling event.
  await act(async () => {
    cardActions()[0]!.dispatchEvent(
      new FocusEvent("focusout", { bubbles: true, relatedTarget: outside }),
    );
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
  expect(card()).toBeNull();
});

it("skips a non-row element when handing focus to the next row", async () => {
  await mount("/repo/hover-decoy");
  const rows = historyRows();

  // A separator or group heading between two rows used to end the traversal
  // silently: `nextElementSibling` found the decoy, it had no button, and Tab
  // dropped out of the list entirely.
  const decoy = document.createElement("li");
  decoy.textContent = "last week";
  rows[0]!.closest("li")!.after(decoy);

  await focusRow(rows[0]!);
  key(rows[0]!, { key: "Tab" });
  const last = cardActions().at(-1)!;
  act(() => last.focus());
  key(last, { key: "Tab" });

  expect(document.activeElement).toBe(rows[1]);
});

it("lets the final row's card give Tab back to the browser", async () => {
  await mount("/repo/hover-last-row");
  const row = historyRows().at(-1)!;

  await focusRow(row);
  key(row, { key: "Tab" });
  const last = cardActions().at(-1)!;
  act(() => last.focus());

  const event = new KeyboardEvent("keydown", {
    key: "Tab",
    bubbles: true,
    cancelable: true,
  });
  act(() => {
    last.dispatchEvent(event);
  });

  // Not prevented, so the browser moves focus onward instead of trapping the
  // user on the last row.
  expect(event.defaultPrevented).toBe(false);
});

it("marks the open commit as the current one, not a pressed toggle", async () => {
  await mount("/repo/hover-aria", commits[0]!.sha);
  const rows = historyRows();

  // aria-pressed would conflict with aria-expanded on the same button.
  expect(rows[0]!.hasAttribute("aria-pressed")).toBe(false);
  expect(rows[0]!.getAttribute("aria-current")).toBe("true");
  expect(rows[1]!.hasAttribute("aria-current")).toBe(false);

  expect(rows[0]!.getAttribute("aria-haspopup")).toBe("dialog");
  expect(rows[0]!.getAttribute("aria-expanded")).toBe("false");
  expect(rows[0]!.hasAttribute("aria-controls")).toBe(false);

  await focusRow(rows[0]!);
  expect(rows[0]!.getAttribute("aria-expanded")).toBe("true");
  const controlled = rows[0]!.getAttribute("aria-controls");
  expect(controlled).toBe(card()!.id);
});

it("waits VS Code's 300ms before opening on hover", async () => {
  await mount("/repo/hover-delay");
  const row = historyRows()[0]!;

  // Mounted and fetched with real timers, so only the hover is on a fake
  // clock. `editor.hover.delay` is 300 in VS Code; this row used to wait 600,
  // which made a deliberate hover feel sluggish.
  vi.useFakeTimers();
  act(() => {
    row.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
  });

  act(() => {
    vi.advanceTimersByTime(299);
  });
  expect(card()).toBeNull();

  act(() => {
    vi.advanceTimersByTime(1);
  });
  await act(async () => {
    await Promise.resolve();
  });
  expect(card()).not.toBeNull();
});

it("does not report the row as expanded until the card has painted", async () => {
  await mount("/repo/hover-pending");
  const row = historyRows()[0]!;

  act(() => row.focus());

  // The card withholds its paint until the commit message arrives, so `open`
  // means a card is wanted, not that one is on screen. Reporting
  // `aria-expanded` from it would tell a screen reader the row is expanded
  // during the reveal gap, pointing at an id that does not exist yet.
  expect(row.getAttribute("aria-expanded")).toBe("false");
  expect(row.hasAttribute("aria-controls")).toBe(false);
  expect(card()).toBeNull();

  // Once the card paints, the row reports it — and the id it points at is the
  // card that is actually on screen.
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
  expect(card()).not.toBeNull();
  expect(row.getAttribute("aria-expanded")).toBe("true");
  expect(row.getAttribute("aria-controls")).toBe(card()!.id);
});

it("drops the expanded state again when the card closes", async () => {
  await mount("/repo/hover-collapse");
  const row = historyRows()[0]!;
  await focusRow(row);
  expect(row.getAttribute("aria-expanded")).toBe("true");

  vi.useFakeTimers();
  act(() => {
    row.dispatchEvent(new MouseEvent("mouseout", { bubbles: true }));
  });
  act(() => {
    vi.advanceTimersByTime(299);
  });
  expect(card()).not.toBeNull();

  act(() => {
    vi.advanceTimersByTime(1);
  });
  expect(card()).toBeNull();
  expect(row.getAttribute("aria-expanded")).toBe("false");
  expect(row.hasAttribute("aria-controls")).toBe(false);
});
