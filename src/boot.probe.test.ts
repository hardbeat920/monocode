// @vitest-environment happy-dom
import { describe, expect, it } from "vitest";

// Boot probe: reproduce the stuck splash by importing the real entry module.
// A module-evaluation crash (before main.tsx's body runs) is exactly what
// leaves the splash up with no error box in either the webview or a browser.
describe("boot", () => {
  it("entry module evaluates", async () => {
    await expect(import("./main")).resolves.toBeTruthy();
  });
});
