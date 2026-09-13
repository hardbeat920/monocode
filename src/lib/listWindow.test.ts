import { describe, expect, it } from "vitest";
import { LIST_PAGE_SIZE, listWindowSize } from "./listWindow";

describe("listWindowSize", () => {
  it("returns 0 for an empty list", () => {
    expect(listWindowSize(0, LIST_PAGE_SIZE)).toBe(0);
  });

  it("returns the full list when it fits in one page", () => {
    expect(listWindowSize(8, LIST_PAGE_SIZE)).toBe(8);
  });

  it("caps the first page", () => {
    expect(listWindowSize(200, LIST_PAGE_SIZE)).toBe(LIST_PAGE_SIZE);
  });

  it("grows as more items are requested", () => {
    expect(listWindowSize(200, LIST_PAGE_SIZE * 2)).toBe(LIST_PAGE_SIZE * 2);
  });

  it("cannot grow past the list", () => {
    expect(listWindowSize(40, 200)).toBe(40);
  });

  it("expands far enough to include a required item", () => {
    expect(listWindowSize(200, LIST_PAGE_SIZE, 80)).toBe(81);
  });
});
