import { describe, expect, it } from "vitest";
import { GRID_GAMES, stepSlider } from "./gridGames";

describe("grid game slider", () => {
  it("lists each game once, under a stable id", () => {
    const ids = GRID_GAMES.map((game) => game.id);
    expect(ids.length).toBeGreaterThanOrEqual(2);
    expect(new Set(ids).size).toBe(ids.length);
    expect(ids).toContain("pacman");
    expect(ids).toContain("snake");
  });

  it("walks to the end and back instead of wrapping", () => {
    const count = 3;
    const steps: number[] = [0];
    let index = 0;
    let dir: 1 | -1 = 1;
    for (let i = 0; i < 6; i++) {
      ({ index, dir } = stepSlider(index, dir, count));
      steps.push(index);
    }
    expect(steps).toEqual([0, 1, 2, 1, 0, 1, 2]);
  });

  it("pings between two games", () => {
    let index = 0;
    let dir: 1 | -1 = 1;
    const seen: number[] = [];
    for (let i = 0; i < 4; i++) {
      ({ index, dir } = stepSlider(index, dir, 2));
      seen.push(index);
    }
    expect(seen).toEqual([1, 0, 1, 0]);
  });

  it("stays put when there is only one game", () => {
    expect(stepSlider(0, 1, 1)).toEqual({ index: 0, dir: 1 });
    expect(stepSlider(0, -1, 0)).toEqual({ index: 0, dir: 1 });
  });

  it("boots every catalogued game onto a board", () => {
    for (const game of GRID_GAMES) {
      const arcade = game.create();
      arcade.resize(172, 28);
      arcade.step(33);
      expect(arcade.fade()).toBeGreaterThan(0);
      expect(arcade.controlled()).toBe(false);
    }
  });
});
