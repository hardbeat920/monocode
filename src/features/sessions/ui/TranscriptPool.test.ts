// @vitest-environment happy-dom
import { act, createElement, StrictMode, useEffect, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  TRANSCRIPT_POOL_LIMIT,
  PooledTranscript,
  TranscriptPool,
  TranscriptPoolOutlet,
} from "./TranscriptPool";

let container: HTMLDivElement;
let root: Root;
let mounts: string[];
let unmounts: string[];

function Probe({
  id,
  visible,
  parked,
}: {
  id: string;
  visible?: boolean;
  parked?: boolean;
  blocks?: object[];
}) {
  const [instance] = useState(() => Math.random());
  useEffect(() => {
    mounts.push(id);
    return () => {
      unmounts.push(id);
    };
  }, [id]);
  return createElement("div", {
    "data-probe": id,
    "data-instance": instance,
    "data-visible": String(visible),
    "data-parked": String(!!parked),
  });
}

function render(
  pool: TranscriptPool,
  shown: string | null,
  onFocus = vi.fn(),
  strict = false,
) {
  act(() =>
    root.render(
      createElement(
        strict ? StrictMode : "div",
        null,
        shown
          ? createElement(
              "section",
              { key: shown, "data-pane": shown },
              createElement(
                PooledTranscript,
                { pool, sessionId: shown, onMouseDown: onFocus },
                createElement(Probe, { id: shown, visible: true }),
              ),
            )
          : null,
        createElement(TranscriptPoolOutlet, { pool }),
      ),
    ),
  );
}

function probe(id: string) {
  return document.querySelector<HTMLElement>(`[data-probe="${id}"]`);
}

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  mounts = [];
  unmounts = [];
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

describe("transcript pool", () => {
  it("cycles through a full pool of chats without rebuilding any transcript", () => {
    const pool = new TranscriptPool();
    // A revisit parks the outgoing chat before reclaiming its own entry.
    const ids = Array.from(
      { length: TRANSCRIPT_POOL_LIMIT },
      (_, index) => `chat-${index}`,
    );
    const instances = new Map<string, string | undefined>();
    for (const id of ids) {
      render(pool, id);
      instances.set(id, probe(id)?.dataset.instance);
    }
    for (const id of ids) {
      render(pool, id);
      expect(probe(id)?.dataset.instance).toBe(instances.get(id));
    }
    expect(mounts).toEqual(ids);
    expect(unmounts).toEqual([]);
  });

  it("does not replay a transcript's effects when switching back to it", () => {
    // StrictMode replays the effects of any component React places, and a
    // reordered portal counts as placed, so the outlet must keep its order.
    const pool = new TranscriptPool();
    for (const id of ["a", "b", "c"]) render(pool, id, vi.fn(), true);
    mounts = [];
    unmounts = [];
    for (const id of ["a", "c", "b", "a"]) render(pool, id, vi.fn(), true);

    expect(mounts).toEqual([]);
    expect(unmounts).toEqual([]);
  });

  it("shows the transcript inside the pane that hosts it", () => {
    const pool = new TranscriptPool();
    render(pool, "a");

    const shown = probe("a");
    expect(shown?.closest('[data-pane="a"]')).not.toBeNull();
    expect(shown?.dataset.visible).toBe("true");
  });

  it("keeps the snapshot stable when a pane supplies equivalent transcript props", () => {
    const pool = new TranscriptPool();
    const onFocus = vi.fn();
    render(pool, "a", onFocus);
    const instance = probe("a")?.dataset.instance;
    const snapshot = pool.getSnapshot();
    const changed = vi.fn();
    const unsubscribe = pool.subscribe(changed);
    for (let update = 0; update < 20; update++) render(pool, "a", onFocus);
    expect(changed).not.toHaveBeenCalled();
    expect(pool.getSnapshot()).toBe(snapshot);
    expect(probe("a")?.dataset.instance).toBe(instance);
    unsubscribe();
  });

  it("updates the portal's event handler when the pane supplies a new callback", () => {
    const pool = new TranscriptPool();
    const before = vi.fn();
    const after = vi.fn();
    render(pool, "a", before);
    render(pool, "a", after);
    act(() =>
      probe("a")?.dispatchEvent(new MouseEvent("mousedown", { bubbles: true })),
    );
    expect(before).not.toHaveBeenCalled();
    expect(after).toHaveBeenCalledOnce();
  });

  it("detaches hidden DOM and restores the same mounted transcript on activation", () => {
    const pool = new TranscriptPool();
    render(pool, "a");
    const instance = probe("a")?.dataset.instance;
    const entry = pool.getSnapshot()[0];
    act(() =>
      pool.show(
        "a",
        entry.host!,
        createElement(Probe, { id: "a", visible: false }),
      ),
    );
    expect(probe("a")).toBeNull();
    const hidden =
      entry.container.querySelector<HTMLElement>('[data-probe="a"]');
    expect(hidden?.dataset.visible).toBe("false");
    expect(hidden?.dataset.instance).toBe(instance);
    expect(unmounts).toEqual([]);
    render(pool, "a");
    expect(probe("a")).toBe(hidden);
    expect(probe("a")?.dataset.visible).toBe("true");
    expect(mounts).toEqual(["a"]);
  });

  it("reuses the mounted transcript when a session is shown again", () => {
    const pool = new TranscriptPool();
    render(pool, "a");
    const instance = probe("a")?.dataset.instance;

    render(pool, "b");
    expect(probe("a")).toBeNull();
    expect(unmounts).toEqual([]);

    render(pool, "a");
    expect(probe("a")?.dataset.instance).toBe(instance);
    expect(probe("a")?.closest('[data-pane="a"]')).not.toBeNull();
    expect(mounts).toEqual(["a", "b"]);
  });

  it("marks a parked transcript hidden until a pane shows it again", () => {
    const pool = new TranscriptPool();
    render(pool, "a");
    render(pool, null);

    const parked = pool.getSnapshot()[0];
    expect(parked.host).toBeNull();
    expect(parked.element.props).toMatchObject({
      visible: false,
      parked: true,
    });

    render(pool, "a");
    expect(probe("a")?.dataset.parked).toBe("false");
  });

  it("unmounts the least recently shown transcripts beyond the limit", () => {
    const pool = new TranscriptPool(2);
    for (const id of ["a", "b", "c", "d"]) render(pool, id);
    render(pool, null);

    expect(unmounts).toEqual(["a", "b"]);
    expect(pool.getSnapshot().map((entry) => entry.id)).toEqual(["c", "d"]);
  });

  it("forwards mouse down from the transcript to its pane", () => {
    const pool = new TranscriptPool();
    const onFocus = vi.fn();
    render(pool, "a", onFocus);

    act(() => {
      probe("a")?.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    });
    expect(onFocus).toHaveBeenCalledTimes(1);
  });

  it("ignores a park from a pane that no longer hosts the session", () => {
    const pool = new TranscriptPool();
    const stale = document.createElement("div");
    const host = document.createElement("div");
    const element = createElement(Probe, { id: "a" });
    pool.show("a", stale, element);
    pool.show("a", host, element);

    pool.park("a", stale);
    expect(pool.getSnapshot()[0].host).toBe(host);
    expect(host.childElementCount).toBe(1);
  });

  it("renders in place without a pool", () => {
    act(() =>
      root.render(
        createElement(
          PooledTranscript,
          { sessionId: "a" },
          createElement(Probe, { id: "a", visible: true }),
        ),
      ),
    );
    expect(probe("a")?.parentElement).toBe(container);
  });
});

it("bounds parked DOM across many small chats, without evicting the visible one", () => {
  const pool = new TranscriptPool(12, 4);
  for (const id of ["a", "b", "c", "d"]) render(pool, id);
  const entries = pool.getSnapshot();
  expect(
    entries
      .filter((entry) => !entry.host)
      .reduce((sum, entry) => sum + entry.retainedNodes, 0),
  ).toBeLessThanOrEqual(4);
  expect(probe("d")).not.toBeNull();
  expect(unmounts).toContain("a");
});

it("does not let one oversized DOM evict the other warm transcripts", () => {
  const pool = new TranscriptPool(12, 10);
  render(pool, "small");
  render(pool, "large");
  for (let i = 0; i < 20; i++)
    probe("large")!.append(document.createElement("span"));
  render(pool, "next");
  expect(pool.getSnapshot().map((entry) => entry.id)).toEqual([
    "small",
    "next",
  ]);
  expect(unmounts).toEqual(["large"]);
  render(pool, "small");
  expect(mounts.filter((id) => id === "small")).toHaveLength(1);
});

it("evicts oversized transcript data even when its visible DOM is small", () => {
  const pool = new TranscriptPool(12, 100, 1000);
  render(pool, "large");
  const entry = pool.getSnapshot()[0];
  act(() =>
    pool.show(
      "large",
      entry.host!,
      createElement(Probe, {
        id: "large",
        visible: true,
        blocks: [{ text: "x".repeat(1000) }],
      }),
    ),
  );
  render(pool, "next");
  expect(pool.getSnapshot().map((entry) => entry.id)).toEqual(["next"]);
  expect(unmounts).toContain("large");
});

it("keeps two heavy revisits warm and reduces parked memory during pressure", () => {
  const pool = new TranscriptPool();
  // About 8k nodes each: all three parked during a revisit fit the budget.
  const ids = Array.from({ length: 3 }, (_, index) => `heavy-${index}`);
  for (const id of ids) {
    render(pool, id);
    const fragment = document.createDocumentFragment();
    for (let token = 0; token < 4000; token++) {
      const span = document.createElement("span");
      span.textContent = `token${token}`;
      fragment.append(span);
    }
    probe(id)!.append(fragment);
  }
  for (const id of ids) render(pool, id);
  expect(mounts).toHaveLength(3);
  expect(unmounts).toHaveLength(0);
  act(() => pool.setResourcePressure(true));
  expect(
    pool
      .getSnapshot()
      .filter((entry) => !entry.host)
      .reduce((count, entry) => count + entry.retainedNodes, 0),
  ).toBeLessThanOrEqual(10_000);
  expect(probe(ids[2])).not.toBeNull();
  expect(unmounts.length).toBeGreaterThan(0);
});
