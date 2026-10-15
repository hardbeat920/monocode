// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";
import { chatReferenceAtPoint } from "./chatReferenceHighlights";
import { newChatReference } from "./chatReferences";

afterEach(() => {
  document.body.replaceChildren();
});

function citedRange(text: string, rect: DOMRect) {
  const p = document.createElement("p");
  p.textContent = text;
  document.body.append(p);
  const range = document.createRange();
  range.selectNodeContents(p);
  vi.spyOn(range, "getClientRects").mockReturnValue([
    rect,
  ] as unknown as DOMRectList);
  return range;
}

it("finds the reference whose cited text is under the pointer", () => {
  const first = newChatReference(
    "a",
    "",
    citedRange("a", new DOMRect(0, 0, 50, 10)),
  );
  const second = newChatReference(
    "b",
    "",
    citedRange("b", new DOMRect(0, 20, 50, 10)),
  );
  expect(chatReferenceAtPoint([first, second], 10, 25)).toBe(second);
  expect(chatReferenceAtPoint([first, second], 10, 5)).toBe(first);
  expect(chatReferenceAtPoint([first, second], 200, 5)).toBeNull();
});

it("ignores a reference whose text has left the page", () => {
  const range = citedRange("gone", new DOMRect(0, 0, 50, 10));
  const reference = newChatReference("gone", "", range);
  document.body.replaceChildren();
  expect(chatReferenceAtPoint([reference], 10, 5)).toBeNull();
});
