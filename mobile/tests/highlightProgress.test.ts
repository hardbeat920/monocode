import { describe, expect, it } from "vitest";

import { HIGHLIGHT_POLL_MS, HighlightProgress } from "@/features/files/highlightProgress";

const reply = (count: number, palette = ["#fff"]) => ({
  palette,
  lines: Array.from({ length: count }, () => [["x", 0]] as [string, number][]),
});

describe("HighlightProgress", () => {
  it("asks four times a second, as the original did", () => {
    expect(HIGHLIGHT_POLL_MS).toBe(250);
  });

  it("shows every reply that brought colours, so the file fills in at the original pace", () => {
    const progress = new HighlightProgress();
    let published = 0;
    for (let i = 0; i < 40; i++) {
      progress.add(reply(100));
      if (progress.take()) published++;
    }
    expect(published).toBe(40);
    expect(progress.collected).toBe(4000);
  });

  it("hands over nothing for a reply with nothing new", () => {
    const progress = new HighlightProgress();
    progress.add(reply(3));
    expect(progress.take()?.lines.length).toBe(3);
    progress.add({ palette: ["#fff"], lines: [] });
    // The original rendered the whole file again only when lines or palette changed too.
    expect(progress.take()).toBeNull();
    progress.add({ palette: ["#fff", "#000"], lines: [] });
    expect(progress.take()?.palette.length).toBe(2);
  });

  it("collects in place: a reply never copies the lines before it", () => {
    const progress = new HighlightProgress();
    progress.add(reply(1));
    const first = progress.take()!;
    progress.add(reply(1));
    const second = progress.take()!;
    // One array grows; the object handed over is new each time, so the screen re-renders.
    expect(second.lines).toBe(first.lines);
    expect(second).not.toBe(first);
    expect(second.lines.length).toBe(2);
  });
});
