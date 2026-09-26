// @vitest-environment happy-dom
import { describe, expect, it } from "vitest";
import {
  transcriptMutationNeedsRepaint,
  transcriptWordRanges,
} from "./transcriptHighlights";

describe("transcriptMutationNeedsRepaint", () => {
  function record(
    target: Node,
    extra: Partial<MutationRecord> = {},
  ): MutationRecord {
    return { target, removedNodes: [], ...extra } as MutationRecord;
  }

  it("ignores streaming changes in items that cannot contain the query", () => {
    const root = document.createElement("div");
    root.innerHTML = `<div data-transcript-search-item><p>unrelated output</p></div>`;
    const item = root.firstElementChild as HTMLElement;

    expect(transcriptMutationNeedsRepaint([record(item)], "needle")).toBe(false);
  });

  it("repaints when a mutated item contains the query", () => {
    const root = document.createElement("div");
    root.innerHTML = `<div data-transcript-search-item><p>found the NEEDLE here</p></div>`;
    const text = root.querySelector("p")?.firstChild as Text;

    expect(transcriptMutationNeedsRepaint([record(text)], "needle")).toBe(true);
  });

  it("repaints structural changes outside a searchable item", () => {
    const root = document.createElement("div");

    expect(transcriptMutationNeedsRepaint([record(root)], "needle")).toBe(true);
  });

  it("repaints when an in-place text change removes the query", () => {
    const root = document.createElement("div");
    root.innerHTML = `<div data-transcript-search-item><p>all done</p></div>`;
    const text = root.querySelector("p")?.firstChild as Text;

    expect(
      transcriptMutationNeedsRepaint(
        [record(text, { type: "characterData", oldValue: "deploy failed" })],
        "failed",
      ),
    ).toBe(true);
  });

  it("repaints when a character edit breaks a cross-node match", () => {
    const root = document.createElement("div");
    root.innerHTML = `
      <div data-transcript-search-item>
        <p><span>nee</span><span>dle</span></p>
      </div>
    `;
    const text = root.querySelector("span")?.firstChild as Text;

    expect(
      transcriptMutationNeedsRepaint(
        [record(text, { type: "characterData", oldValue: "nee" })],
        "needle",
      ),
    ).toBe(true);
  });

  it("repaints node removals that could drop a match", () => {
    const root = document.createElement("div");
    const item = document.createElement("div");
    item.setAttribute("data-transcript-search-item", "");
    root.append(item);

    expect(
      transcriptMutationNeedsRepaint(
        [record(item, { type: "childList", removedNodes: [document.createTextNode("needle")] })],
        "needle",
      ),
    ).toBe(true);
  });

  it("uses the same unicode case folding as the highlighter", () => {
    const root = document.createElement("div");
    root.innerHTML = `<div data-transcript-search-item><p>ſ</p></div>`;

    expect(transcriptMutationNeedsRepaint([record(root.firstChild!)], "s")).toBe(
      true,
    );
  });

  it("does nothing without a query", () => {
    const root = document.createElement("div");
    expect(transcriptMutationNeedsRepaint([record(root)], "  ")).toBe(false);
  });
});

describe("transcriptWordRanges", () => {
  it("highlights only matching words, including text split by formatting", () => {
    const root = document.createElement("div");
    root.innerHTML = `
      <div data-transcript-search-item>
        <pre data-selectable-agent-response="one">Hey can you check the notes?</pre>
      </div>
      <div data-transcript-search-item data-transcript-search-current="true">
        <div data-selectable-agent-response="two"><p>Hey <strong>can</strong> you review it?</p></div>
        <button>Hey can you</button>
      </div>
    `;

    const result = transcriptWordRanges(root, "hey can you");
    expect(result.matches.map((range) => range.toString())).toEqual([
      "Hey can you",
      "Hey can you",
    ]);
    expect(result.current).toBe(result.matches[1]);
  });

  it("treats punctuation in a query literally", () => {
    const root = document.createElement("div");
    root.innerHTML = `<div data-transcript-search-item><pre data-selectable-agent-response="one">foo.bar fooXbar</pre></div>`;
    expect(
      transcriptWordRanges(root, "foo.bar").matches.map((range) =>
        range.toString(),
      ),
    ).toEqual(["foo.bar"]);
  });
});
