// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  captureDraft,
  dropPastedText,
  insertRestoredText,
} from "./draftRestore";

/** Selection is only honoured on an attached field, as in a real document. */
const attached: HTMLTextAreaElement[] = [];

function field(value = "", start?: number, end?: number) {
  const el = document.createElement("textarea");
  document.body.append(el);
  attached.push(el);
  el.value = value;
  el.setSelectionRange(start ?? value.length, end ?? start ?? value.length);
  return el;
}

afterEach(() => {
  for (const el of attached.splice(0)) el.remove();
});

describe("captureDraft", () => {
  it("records the value and selection a paste landed against", () => {
    expect(captureDraft(field("hello world", 6, 11))).toMatchObject({
      value: "hello world",
      start: 6,
      end: 11,
    });
  });

  it("ignores a target that is not a text field", () => {
    expect(captureDraft(document.createElement("div"))).toBeNull();
    expect(captureDraft(null)).toBeNull();
  });
});

describe("dropPastedText", () => {
  it("removes a URI inserted at the captured caret and keeps a later suffix", () => {
    const el = field("keep", 4, 4);
    const captured = captureDraft(el)!;
    el.setRangeText("file:///tmp/a.png", 4, 4, "end");
    el.value += "!";
    dropPastedText(captured, "file:///tmp/a.png");

    expect(el.value).toBe("keep!");
  });

  it("removes a URI the webview inserted before the paste was observed", () => {
    const uri = "file:///tmp/a.png";
    const el = field(uri, uri.length, uri.length);
    const captured = captureDraft(el)!;
    dropPastedText(captured, uri);

    expect(el.value).toBe("");
  });
});

describe("insertRestoredText", () => {
  it("restores at the range the draft had, replacing only that range", () => {
    const el = field("hello world", 6, 11);
    const captured = captureDraft(el)!;
    insertRestoredText(captured, "URI");

    expect(el.value).toBe("hello URI");
  });

  it("inserts at the caret rather than clobbering a later selection", () => {
    const el = field("hello world", 6, 11);
    const captured = captureDraft(el)!;
    // The draft moved on while the clipboard read was in flight.
    el.value = "hello brave world";
    el.setSelectionRange(6, 11);
    insertRestoredText(captured, "URI");

    // "brave" survives: reusing the current range would have eaten it.
    expect(el.value).toBe("hello URIbrave world");
  });

  it("still inserts when the draft changed and the caret is at the end", () => {
    const el = field("", 0, 0);
    const captured = captureDraft(el)!;
    el.value = "typed meanwhile";
    el.setSelectionRange(15, 15);
    insertRestoredText(captured, "URI");

    expect(el.value).toBe("typed meanwhileURI");
  });

  it("does not insert a paste the field already contains", () => {
    const el = field("hello world", 6, 11);
    const captured = captureDraft(el)!;
    el.value = "hello URI";
    el.setSelectionRange(9, 9);
    insertRestoredText(captured, "URI");

    expect(el.value).toBe("hello URI");
  });

  it("announces the change so React sees it", () => {
    const el = field("");
    const onInput = vi.fn();
    el.addEventListener("input", onInput);
    insertRestoredText(captureDraft(el)!, "URI");

    expect(onInput).toHaveBeenCalledTimes(1);
  });
});
