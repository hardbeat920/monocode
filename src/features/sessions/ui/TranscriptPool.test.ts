// @vitest-environment happy-dom
import { act, createElement, useEffect, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
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

function render(pool: TranscriptPool, shown: string | null, onFocus = vi.fn()) {
  act(() =>
    root.render(
      createElement(
        "div",
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
  it("shows the transcript inside the pane that hosts it", () => {
    const pool = new TranscriptPool();
    render(pool, "a");

    const shown = probe("a");
    expect(shown?.closest('[data-pane="a"]')).not.toBeNull();
    expect(shown?.dataset.visible).toBe("true");
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
