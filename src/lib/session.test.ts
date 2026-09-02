import { describe, expect, it } from "vitest";
import { harnessSupportsAttachments } from "./session";

describe("provider capabilities", () => {
  it("keeps Antigravity attachments enabled for staged file references", () => {
    expect(harnessSupportsAttachments("antigravity")).toBe(true);
  });
});
