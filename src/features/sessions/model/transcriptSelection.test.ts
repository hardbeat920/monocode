import { describe, expect, it } from "vitest";
import { validateTranscriptSelection } from "./transcriptSelection";

describe("validateTranscriptSelection", () => {
  it("accepts non-empty text within one settled response", () => {
    expect(
      validateTranscriptSelection({
        text: " selected ",
        collapsed: false,
        anchorResponseId: "response-1",
        focusResponseId: "response-1",
      }),
    ).toBe("selected");
  });

  it.each([
    [true, "response-1", "response-1"],
    [false, null, "response-1"],
    [false, "response-1", null],
    [false, "response-1", "response-2"],
  ])("rejects an invalid selection %#", (collapsed, anchor, focus) => {
    expect(
      validateTranscriptSelection({
        text: "selected",
        collapsed,
        anchorResponseId: anchor,
        focusResponseId: focus,
      }),
    ).toBeNull();
  });
});
