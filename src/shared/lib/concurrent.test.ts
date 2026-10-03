import { describe, expect, it } from "vitest";
import { forEachConcurrent, runSequential } from "./concurrent";

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

describe("runSequential", () => {
  it("runs one item at a time, in order", async () => {
    const log: string[] = [];
    let running = 0;
    await runSequential(["a", "b", "c"], async (item) => {
      running += 1;
      expect(running).toBe(1);
      log.push(item);
      await Promise.resolve();
      running -= 1;
    });
    expect(log).toEqual(["a", "b", "c"]);
  });

  it("continues past failures and reports them", async () => {
    const error = new Error("locked");
    const result = await runSequential([1, 2, 3], async (item) => {
      if (item === 2) throw error;
    });
    expect(result).toEqual({ done: [1, 3], failed: [{ item: 2, error }] });
  });
});
