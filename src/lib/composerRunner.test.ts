import { describe, expect, it } from "vitest";
import {
  COIN_FIRST_MAX_MS,
  COIN_FIRST_MIN_MS,
  COIN_GAP_MAX_MS,
  COIN_GAP_MIN_MS,
  COIN_HOVER,
  CRASH_RECOIL_MS,
  CRASH_RECOIL_PX,
  CRASH_SHAKE_MS,
  CRASH_STUN_MS,
  EXIT_PEAK,
  EXIT_SINK,
  JUMP_LEAD,
  RUNNER_INSET,
  RUNNER_SIZE,
  STAR_COUNT,
  coinCollected,
  exitJumpY,
  hitsChevron,
  jumpHeight,
  nextCoinDelay,
  obstacleFromRects,
  pickCoinX,
  pingPong,
  poseAt,
  recoilAlong,
  runnerPose,
  runnerTrack,
  scaleTrackX,
  spriteClipBottom,
  stepAlong,
  stunDone,
  stunShake,
  stunStars,
} from "./composerRunner";

const BOX = { left: 100, right: 500, top: 200, bottom: 320, width: 400 };

describe("composerRunner", () => {
  it("runs right, then flips and runs back", () => {
    expect(pingPong(0, 100)).toEqual({ t: 0, facing: 1 });
    expect(pingPong(40, 100)).toEqual({ t: 40, facing: 1 });
    expect(pingPong(100, 100)).toEqual({ t: 100, facing: 1 });
    expect(pingPong(140, 100)).toEqual({ t: 60, facing: -1 });
    expect(pingPong(199.9, 100).facing).toBe(-1);
    expect(pingPong(199.9, 100).t).toBeCloseTo(0.1);
    expect(pingPong(200, 100)).toEqual({ t: 0, facing: 1 });
    expect(pingPong(240, 100)).toEqual({ t: 40, facing: 1 });
  });

  it("wraps negative distance so a restart still faces forward", () => {
    expect(pingPong(-10, 100)).toEqual({ t: 10, facing: -1 });
  });

  it("parks at the left inset when the track is too short", () => {
    expect(runnerPose(80, 8, null)).toEqual({
      x: RUNNER_INSET,
      y: 0,
      facing: 1,
      airborne: false,
    });
  });

  it("places x in box coordinates, inset from each end", () => {
    const right = runnerPose(0, 200, null);
    expect(right.x).toBe(RUNNER_INSET);
    expect(right.facing).toBe(1);
    expect(right.y).toBe(0);

    const far = runnerPose(200 - RUNNER_INSET * 2, 200, null);
    expect(far.x).toBe(200 - RUNNER_INSET);
    expect(far.facing).toBe(1);
  });

  it("jumps a parabola over the hurdle and lands after it", () => {
    const obstacle = { left: 80, right: 104, height: 40 };
    const mid = (obstacle.left + obstacle.right) / 2;

    expect(jumpHeight(obstacle.left - JUMP_LEAD, obstacle)).toBe(0);
    expect(jumpHeight(obstacle.right + JUMP_LEAD, obstacle)).toBe(0);
    expect(jumpHeight(mid, obstacle)).toBeCloseTo(40);
    expect(jumpHeight(mid, obstacle)).toBeGreaterThan(
      jumpHeight(obstacle.left, obstacle),
    );

    const peak = runnerPose(mid - RUNNER_INSET, 400, obstacle);
    expect(peak.airborne).toBe(true);
    expect(peak.y).toBeCloseTo(40);

    const before = runnerPose(0, 400, obstacle);
    expect(before.airborne).toBe(false);
    expect(before.y).toBe(0);
  });

  it("keeps the landing arc after the coin is grabbed", () => {
    const coins = [{ id: 1, x: 120, height: COIN_HOVER }];
    const atCoin = runnerPose(120 - RUNNER_INSET, 400, null, coins);
    const past = runnerPose(120 - RUNNER_INSET + 20, 400, null, coins);
    expect(atCoin.y).toBeGreaterThan(past.y);
    expect(past.y).toBeGreaterThan(0);
    expect(runnerPose(120 - RUNNER_INSET + 20, 400, null, []).y).toBe(0);
  });

  it("collects only when the sprite overlaps the coin", () => {
    const coins = [{ id: 1, x: 120, height: COIN_HOVER }];
    const peak = runnerPose(120 - RUNNER_INSET, 400, null, coins);
    expect(coinCollected(peak, coins[0])).toBe(true);
    expect(
      coinCollected({ x: 120, y: 8, facing: 1, airborne: true }, coins[0]),
    ).toBe(false);
    expect(
      coinCollected({ x: 40, y: peak.y, facing: 1, airborne: true }, coins[0]),
    ).toBe(false);
  });

  it("spaces coins across the track and away from the runner", () => {
    const values = [0.5, 0.1];
    let i = 0;
    const x = pickCoinX(400, 200, null, () => values[Math.min(i++, 1)] ?? 0);
    expect(x).toBeGreaterThan(50);
    expect(x).toBeLessThan(100);
    expect(pickCoinX(40, 10, null)).toBeNull();
  });

  it("waits several seconds between coins", () => {
    expect(nextCoinDelay(true, () => 0)).toBe(COIN_FIRST_MIN_MS);
    expect(nextCoinDelay(true, () => 1)).toBe(COIN_FIRST_MAX_MS);
    expect(nextCoinDelay(false, () => 0)).toBe(COIN_GAP_MIN_MS);
    expect(nextCoinDelay(false, () => 1)).toBe(COIN_GAP_MAX_MS);
    expect(COIN_GAP_MIN_MS).toBeGreaterThanOrEqual(6000);
  });

  it("hops up then drops behind the rim on the way out", () => {
    expect(exitJumpY(0)).toBe(0);
    expect(exitJumpY(0.38)).toBeCloseTo(EXIT_PEAK);
    expect(exitJumpY(1)).toBe(-EXIT_SINK);
    expect(exitJumpY(0.2)).toBeGreaterThan(0);
    expect(exitJumpY(0.2)).toBeLessThan(EXIT_PEAK);
    expect(exitJumpY(0.9)).toBeLessThan(0);
    expect(spriteClipBottom(8)).toBe(0);
    expect(spriteClipBottom(-4)).toBe(4);
    expect(spriteClipBottom(-40)).toBe(16);
  });

  it("ignores a control that is not sitting on the top border", () => {
    expect(
      obstacleFromRects(BOX, {
        left: 250,
        right: 274,
        top: 40,
        bottom: 64,
      }),
    ).toBeNull();
    expect(
      obstacleFromRects(BOX, {
        left: 10,
        right: 34,
        top: 188,
        bottom: 212,
      }),
    ).toBeNull();
  });

  it("reads the jump-to-latest chevron as a hurdle in box space", () => {
    const hurdle = obstacleFromRects(BOX, {
      left: 288,
      right: 312,
      top: 168,
      bottom: 192,
    });
    expect(hurdle).toEqual({
      left: 188,
      right: 212,
      height: 42,
    });
  });

  it("crashes into the chevron only on the first approach", () => {
    const hurdle = { left: 80, right: 104, height: 40 };
    const half = RUNNER_SIZE / 2;

    expect(hitsChevron(80 - half - 2, 0, 1, hurdle, false)).toBe(false);
    expect(hitsChevron(80 - half, 0, 1, hurdle, false)).toBe(true);
    expect(hitsChevron(120, 0, 1, hurdle, false)).toBe(false);
    expect(hitsChevron(104 + half, 0, -1, hurdle, false)).toBe(true);
    expect(hitsChevron(50, 0, -1, hurdle, false)).toBe(false);
    expect(hitsChevron(80 - half, 8, 1, hurdle, false)).toBe(false);
    expect(hitsChevron(80 - half, 0, 1, hurdle, true)).toBe(false);
    expect(hitsChevron(80 - half, 0, 1, null, false)).toBe(false);
  });

  it("knocks the mascot back, shakes, then finishes the stun", () => {
    expect(recoilAlong(70, 1, 0, 200)).toBe(70);
    expect(recoilAlong(70, 1, CRASH_RECOIL_MS, 200)).toBe(70 - CRASH_RECOIL_PX);
    expect(recoilAlong(70, -1, CRASH_RECOIL_MS, 200)).toBe(70 + CRASH_RECOIL_PX);
    expect(recoilAlong(4, 1, CRASH_RECOIL_MS, 200)).toBe(0);
    expect(recoilAlong(190, -1, CRASH_RECOIL_MS, 200)).toBe(200);

    expect(stunShake(0)).toEqual({ x: 0, y: 0 });
    const wobble = [40, 80, 120, 160].map(stunShake);
    expect(wobble.some((shake) => Math.abs(shake.x) >= 2)).toBe(true);
    expect(wobble.some((shake) => Math.abs(shake.y) >= 1)).toBe(true);
    expect(stunShake(CRASH_SHAKE_MS)).toEqual({ x: 0, y: 0 });
    expect(stunDone(CRASH_STUN_MS - 1)).toBe(false);
    expect(stunDone(CRASH_STUN_MS)).toBe(true);
  });

  it("orbits pixel stars around the sprite for the stun, then clears them", () => {
    const start = stunStars(0);
    expect(start).toHaveLength(STAR_COUNT);
    expect(start[0].opacity).toBe(1);
    expect(new Set(start.map((star) => `${star.dx},${star.dy}`)).size).toBe(
      STAR_COUNT,
    );

    const spun = stunStars(140);
    expect(spun[0].dx).not.toBe(start[0].dx);
    expect(stunStars(CRASH_STUN_MS - 1)[0].opacity).toBeLessThan(1);
    expect(stunStars(CRASH_STUN_MS)).toEqual([]);
  });

  it("runs on the review bar when it is sitting on the composer", () => {
    expect(runnerTrack(BOX, null)).toEqual({
      left: 100,
      top: 200,
      width: 400,
    });
    expect(
      runnerTrack(BOX, {
        left: 108,
        right: 492,
        top: 168,
        bottom: 200,
        width: 384,
      }),
    ).toEqual({ left: 108, top: 168, width: 384 });
  });

  it("keeps relative position when the track width changes", () => {
    expect(scaleTrackX(200, 400, 200)).toBe(100);
    expect(scaleTrackX(0, 400, 800)).toBe(0);
    expect(scaleTrackX(50, 0, 400)).toBe(0);

    const stepped = stepAlong(0, 1, 1000, 100, 40);
    expect(stepped.along).toBe(40);
    expect(stepped.facing).toBe(1);

    const turned = stepAlong(100, 1, 1000, 100, 40);
    expect(turned.along).toBe(100);
    expect(turned.facing).toBe(-1);

    const pose = poseAt(50, 1, 200, null);
    expect(pose.x).toBe(RUNNER_INSET + 50);
    expect(pose.facing).toBe(1);
    expect(pose.y).toBe(0);
  });
});
