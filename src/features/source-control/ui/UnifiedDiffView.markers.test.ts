// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

vi.mock("../../files/editor/syntaxTokens", () => ({
  highlightDiffFile: vi.fn(() => Promise.resolve(null)),
}));

import { buildUnifiedFile } from "../model/unifiedDiff";
import { UnifiedDiffView } from "./UnifiedDiffView";

let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(async () => {
  await act(async () => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

it("marks added and removed lines with a glyph, not only color", async () => {
  const diff = buildUnifiedFile("alpha\nbeta\ngamma\n", "alpha\nBETA\ngamma\n");
  await act(async () =>
    root.render(
      createElement(UnifiedDiffView, {
        files: [
          {
            id: "a.ts",
            path: "a.ts",
            label: "a.ts",
            additions: diff.additions,
            deletions: diff.deletions,
            blocks: diff.blocks,
          },
        ],
      }),
    ),
  );

  // The code span holds only the line text, so copying it skips the marker
  // column and its screen-reader cue.
  const gutters = new Map(
    [...container.querySelectorAll("[data-diff-gutter-row]")].map((row) => [
      row.getAttribute("data-diff-gutter-row"),
      row,
    ]),
  );
  const rows = [...container.querySelectorAll("[data-diff-line]")].map(
    (row) => ({
      glyph: gutters
        .get(row.getAttribute("data-diff-row"))
        ?.querySelector('[data-diff-marker][aria-hidden="true"]')?.textContent,
      cue: row.querySelector(".sr-only")?.textContent ?? null,
      code: row.querySelector("[data-diff-code]")?.textContent,
    }),
  );
  expect(rows).toEqual([
    { glyph: "", cue: null, code: "alpha" },
    { glyph: "−", cue: "Removed: ", code: "beta" },
    { glyph: "+", cue: "Added: ", code: "BETA" },
    { glyph: "", cue: null, code: "gamma" },
  ]);
});
