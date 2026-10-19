import { describe, expect, it } from "vitest";
import { autoLayout } from "./layout";

// Auto layout should read like the app's flow: each box sits to the right of
// the boxes that point at it, and nothing ends up stacked on another box.
describe("autoLayout", () => {
  const box = (id: string) => ({ id, x: 0, y: 0, width: 160, height: 60 });

  it("puts each box to the right of the boxes pointing at it", () => {
    const [screen, api, db] = autoLayout(
      [box("screen"), box("api"), box("db")],
      [
        { source: "screen", target: "api" },
        { source: "api", target: "db" },
      ],
    );
    expect(screen.x + 160).toBeLessThan(api.x);
    expect(api.x + 160).toBeLessThan(db.x);
  });

  it("never stacks boxes on top of each other", () => {
    const out = autoLayout(
      [box("a"), box("b"), box("c"), box("d")],
      [
        { source: "a", target: "b" },
        { source: "a", target: "c" },
        { source: "a", target: "d" },
      ],
    );
    for (let i = 0; i < out.length; i++)
      for (let j = i + 1; j < out.length; j++) {
        const p = out[i];
        const q = out[j];
        const apart =
          p.x + p.width <= q.x ||
          q.x + q.width <= p.x ||
          p.y + p.height <= q.y ||
          q.y + q.height <= p.y;
        expect(apart).toBe(true);
      }
  });

  it("ignores arrows pointing at boxes that no longer exist", () => {
    expect(() =>
      autoLayout([box("a")], [{ source: "a", target: "gone" }]),
    ).not.toThrow();
  });
});
