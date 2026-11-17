import { describe, expect, it, vi } from "vitest";

import {
  DraftController,
  shortUrl,
  type DraftChrome,
  type DraftSurface,
} from "@/features/drafts/DraftController";
import type { DraftRecord, LiveDraft } from "@/features/drafts/sync";

vi.mock("react-native", () => ({
  AppState: { currentState: "active", addEventListener: () => ({ remove() {} }) },
}));

const live = (over: Partial<LiveDraft> = {}): LiveDraft =>
  ({
    live: true,
    kind: "html",
    title: "Plan",
    rev: 1,
    generation: 1,
    state: {},
    stateRev: 0,
    pins: [],
    pending: [],
    control: false,
    waiting: false,
    page: "<p>hi</p>",
    ...over,
  }) as LiveDraft;

/** Answers each read in turn, then waits until the card is hidden. */
function harness(replies: DraftRecord[]) {
  const reads: Record<string, unknown>[] = [];
  const sent: { method: string; params: Record<string, unknown> }[] = [];
  const chrome: DraftChrome[] = [];
  const loads: [string, number][] = [];
  const scripts: string[] = [];
  const pictures: (string | null)[] = [];
  const queue = [...replies];
  const command = vi.fn(async (method: string, params: Record<string, unknown>) => {
    sent.push({ method, params });
    if (method === "draft.read") {
      reads.push(params);
      const next = queue.shift();
      if (next) return next;
      return new Promise(() => {});
    }
    return live({ page: undefined, rev: 9 });
  }) as never;
  const surface: DraftSurface = {
    chrome: (next) => chrome.push(next),
    load: (html, generation) => loads.push([html, generation]),
    script: (script) => scripts.push(script),
    picture: (uri) => pictures.push(uri),
  };
  const card = new DraftController({
    conversationId: "c",
    card: { draft: "d", title: "Plan" },
    light: false,
    command,
    commandId: () => "id",
    surface,
    alert: () => {},
  });
  return { card, reads, sent, chrome, loads, scripts, pictures };
}

const settle = () => new Promise((done) => setTimeout(done, 0));

describe("a draft's card", () => {
  it("only ever peeks while it follows, so scrolling past opens nothing", async () => {
    const { card, reads } = harness([{ live: false, saved: true }]);
    card.show();
    await settle();
    expect(reads[0]).toMatchObject({ draft: "d", peek: true });
    card.dispose();
  });

  it("offers to run a paused page, and the press is what opens it", async () => {
    const { card, chrome, sent } = harness([{ live: false, saved: true }]);
    card.show();
    await settle();
    expect(chrome.at(-1)).toMatchObject({ mode: "paused", open: "Paused · tap to run" });
    card.press("open");
    await settle();
    const open = sent.filter((call) => call.method === "draft.read").at(-1);
    expect(open?.params.peek).toBeUndefined();
    card.dispose();
  });

  it("says a closed draft was closed", async () => {
    const { card, chrome } = harness([{ live: false, saved: true, dismissed: true }]);
    card.show();
    await settle();
    expect(chrome.at(-1)).toMatchObject({ mode: "closed", status: "Plan was closed" });
    card.dispose();
  });

  it("loads the page once and asks for it again only after losing it", async () => {
    const { card, loads, reads } = harness([live(), live({ rev: 2, page: undefined })]);
    card.show();
    await settle();
    await settle();
    expect(loads).toEqual([["<p>hi</p>", 1]]);
    expect(reads[1]).toMatchObject({ generation: 1, since: 1 });
    card.hide();
    card.lost();
    expect(card).toBeTruthy();
    card.dispose();
  });

  it("ignores what the page says without the person's touch", async () => {
    const { card, sent } = harness([live()]);
    card.show();
    await settle();
    card.loaded(1);
    const note = JSON.stringify({ __mc: 1, k: "n", t: "hello" });
    card.message(note, -Infinity);
    await settle();
    expect(sent.some((call) => call.method === "draft.action")).toBe(false);
    card.message(note, Date.now());
    await settle();
    expect(sent.some((call) => call.method === "draft.action")).toBe(true);
    card.dispose();
  });

  it("takes a scroll that began on the page for passing by, not for use", async () => {
    vi.useFakeTimers();
    try {
      const { card, sent } = harness([live()]);
      card.show();
      await vi.advanceTimersByTimeAsync(0);
      card.loaded(1);
      const said = (a: string) =>
        card.message(JSON.stringify({ __mc: 1, k: "u", a }), Date.now());
      const inputs = () => sent.filter((call) => call.params.action === "input");

      said('clicked "div"');
      card.touchTaken();
      await vi.advanceTimersByTimeAsync(1000);
      expect(inputs()).toEqual([]);

      said('clicked "Buy"');
      await vi.advanceTimersByTimeAsync(1000);
      expect(inputs().map((call) => call.params.actions)).toEqual([['clicked "Buy"']]);
      card.dispose();
    } finally {
      vi.useRealTimers();
    }
  });

  it("offers notes to send and the hand back while the page is in use", async () => {
    const { card, chrome } = harness([
      live({ control: true, pending: [{ note: "a", tag: "p", name: "", x: 0, y: 0 }] }),
    ]);
    card.show();
    await settle();
    expect(chrome.at(-1)).toMatchObject({ mode: "page", notes: 1, handBack: true });
    card.dispose();
  });

  it("names a dev server by its address", () => {
    expect(shortUrl("http://localhost:3000/")).toBe("localhost:3000");
  });
});
