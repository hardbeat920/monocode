import { describe, expect, it } from "vitest";
import { HARNESSES } from "../session";
import { harnessUnavailableHint } from "./availability";

describe("harnessUnavailableHint", () => {
  it("uses the same command-free guidance for every provider", () => {
    for (const harness of HARNESSES) {
      const hint = harnessUnavailableHint(harness);
      expect(hint).toContain(
        "not found. Install it, or restart MonoCode if it is already installed.",
      );
      expect(hint).not.toContain("`");
      expect(hint).not.toContain("curl ");
      expect(hint).not.toContain("npm ");
    }
  });
});
