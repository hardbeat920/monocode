import { setDiagnostics } from "@codemirror/lint";
import { EditorState } from "@codemirror/state";
import { describe, expect, it } from "vitest";
import { diagnosticOverviewTicks, scrollbarMetrics } from "./editorScrollbar";

describe("scrollbarMetrics", () => {
  it("sizes and positions the thumb from the viewport", () => {
    expect(scrollbarMetrics(450, 1_000, 100, 200)).toEqual({
      scrollable: true,
      maxScroll: 900,
      maxThumbTop: 168,
      thumbHeight: 32,
      thumbTop: 84,
    });
  });

  it("fills the track when the document does not overflow", () => {
    expect(scrollbarMetrics(0, 100, 100, 200)).toEqual({
      scrollable: false,
      maxScroll: 0,
      maxThumbTop: 0,
      thumbHeight: 200,
      thumbTop: 0,
    });
  });

  it("clamps a stale scroll offset", () => {
    expect(scrollbarMetrics(2_000, 1_000, 200, 100).thumbTop).toBe(68);
  });
});

describe("diagnosticOverviewTicks", () => {
  it("maps diagnostics to the document and keeps the strongest per line", () => {
    let state = EditorState.create({ doc: "one\ntwo\nthree\nfour" });
    state = state.update(
      setDiagnostics(state, [
        { from: 4, to: 7, severity: "warning", message: "warning" },
        { from: 5, to: 6, severity: "error", message: "error" },
        { from: 14, to: 18, severity: "hint", message: "hint" },
      ]),
    ).state;

    expect(diagnosticOverviewTicks(state)).toEqual([
      { severity: "error", top: 1 / 4, pos: 5, message: "error" },
      { severity: "hint", top: 3 / 4, pos: 14, message: "hint" },
    ]);
  });
});
