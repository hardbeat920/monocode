import { describe, expect, it } from "vitest";
import { supportsWebGL2 } from "./webgl";

describe("supportsWebGL2", () => {
  it("reports true when the probe returns a context", () => {
    expect(supportsWebGL2(() => ({}))).toBe(true);
  });

  it("reports false when the probe returns null", () => {
    expect(supportsWebGL2(() => null)).toBe(false);
  });

  it("reports false when the probe throws", () => {
    expect(
      supportsWebGL2(() => {
        throw new Error("no canvas");
      }),
    ).toBe(false);
  });

  it("reports false without a DOM", () => {
    expect(supportsWebGL2()).toBe(false);
  });
});
