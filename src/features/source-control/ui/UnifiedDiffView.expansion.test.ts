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

const diff = buildUnifiedFile("alpha\n", "beta\n");
const files = ["a.ts", "b.ts", "c.ts"].map((id) => ({
  id,
  path: `/repo/${id}`,
  label: id,
  additions: diff.additions,
  deletions: diff.deletions,
  blocks: diff.blocks,
}));

async function render(focusPath?: string) {
  await act(async () =>
    root.render(
      createElement(UnifiedDiffView, {
        files,
        focusPath,
        initialExpansion: "first",
      }),
    ),
  );
  return [...container.querySelectorAll("[aria-expanded]")].map((toggle) =>
    toggle.getAttribute("aria-expanded"),
  );
}

it("opens only the first file", async () => {
  expect(await render()).toEqual(["true", "false", "false"]);
});

it("also opens a focused file the others fold away from", async () => {
  expect(await render("/repo/c.ts")).toEqual(["true", "false", "true"]);
});
