// @vitest-environment happy-dom
import { describe, expect, it } from "vitest";
import { transcriptWordRanges } from "./transcriptHighlights";

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
