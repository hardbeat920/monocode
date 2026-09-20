import { describe, expect, it } from "vitest";
import { forEachConcurrent } from "./concurrent";

describe("forEachConcurrent", () => {
  it("bounds in-flight work and visits every item", async () => {
    let active = 0;
    let peak = 0;
    const seen: number[] = [];

    await forEachConcurrent([0, 1, 2, 3, 4, 5], 2, async (item) => {
      active += 1;
      peak = Math.max(peak, active);
      await Promise.resolve();
      seen.push(item);
      active -= 1;
    });

    expect(peak).toBe(2);
    expect(seen.sort((a, b) => a - b)).toEqual([0, 1, 2, 3, 4, 5]);
  });

  it("stops assigning new work after cancellation", async () => {
    let running = true;
    const seen: number[] = [];

    await forEachConcurrent(
      [0, 1, 2, 3],
      1,
      async (item) => {
        seen.push(item);
        running = false;
      },
      () => running,
    );

    expect(seen).toEqual([0]);
  });
});
