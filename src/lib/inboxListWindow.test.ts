import { describe, expect, it } from "vitest";
import { INBOX_LIST_PAGE, inboxListWindow } from "./inboxListWindow";

describe("inboxListWindow", () => {
  it("returns 0 for an empty list", () => {
    expect(inboxListWindow(0, INBOX_LIST_PAGE, -1)).toBe(0);
  });

  it("returns the full list when it fits in one page", () => {
    expect(inboxListWindow(8, INBOX_LIST_PAGE, -1)).toBe(8);
  });

  it("caps the first page", () => {
    expect(inboxListWindow(200, INBOX_LIST_PAGE, -1)).toBe(INBOX_LIST_PAGE);
  });

  it("grows as more cards are requested", () => {
    expect(inboxListWindow(200, INBOX_LIST_PAGE * 2, -1)).toBe(
      INBOX_LIST_PAGE * 2,
    );
  });

  it("cannot grow past the list", () => {
    expect(inboxListWindow(40, 200, -1)).toBe(40);
  });

  it("expands far enough to include the selected card", () => {
    expect(inboxListWindow(200, INBOX_LIST_PAGE, 80)).toBe(81);
  });
});
