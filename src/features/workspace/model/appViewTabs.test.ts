import { describe, expect, it } from "vitest";
import {
  nextDisplayedTabAfterClose,
  reconcileTitleTabOrder,
} from "./appViewTabs";

describe("shared tab opening order", () => {
  it("appends a session after app tabs even when workspace tabs insert beside the active session", () => {
    expect(
      reconcileTitleTabOrder(
        ["session-1", "session-2", "app-view:inbox"],
        ["session-1", "session-3", "session-2", "app-view:inbox"],
      ),
    ).toEqual(["session-1", "session-2", "app-view:inbox", "session-3"]);
  });

  it("appends new app tabs after sessions without undoing manual reordering", () => {
    expect(
      reconcileTitleTabOrder(
        ["app-view:inbox", "session-2", "session-1"],
        ["session-1", "session-2", "app-view:inbox", "app-view:notes"],
      ),
    ).toEqual(["app-view:inbox", "session-2", "session-1", "app-view:notes"]);
  });

  it("forgets closed tabs so reopening puts them at the end", () => {
    const closed = reconcileTitleTabOrder(
      ["session-1", "app-view:notes", "session-2"],
      ["session-1", "session-2"],
    );
    expect(closed).toEqual(["session-1", "session-2"]);
    expect(
      reconcileTitleTabOrder(closed, [
        "session-1",
        "session-2",
        "app-view:notes",
      ]),
    ).toEqual(["session-1", "session-2", "app-view:notes"]);
  });

  it("retains the order identity when only the workspace grouping changes", () => {
    const order = ["session-2", "app-view:inbox", "session-1"];
    expect(
      reconcileTitleTabOrder(order, [
        "session-1",
        "session-2",
        "app-view:inbox",
      ]),
    ).toBe(order);
  });
});

describe("closing mixed tabs in visible order", () => {
  const tabs = [
    "session-1",
    "app-view:notes",
    "session-2",
    "app-view:inbox",
  ].map((id) => ({ id }));

  it.each([
    ["session-1", "app-view:notes"],
    ["app-view:notes", "session-2"],
    ["session-2", "app-view:inbox"],
    ["app-view:inbox", "session-2"],
  ])("selects %s's neighbor %s", (closingId, expected) => {
    expect(nextDisplayedTabAfterClose(tabs, closingId, closingId)).toBe(
      expected,
    );
  });

  it("follows the displayed order after dragging an app tab between sessions", () => {
    const reordered = [tabs[3], tabs[0], tabs[1], tabs[2]];
    expect(
      nextDisplayedTabAfterClose(reordered, "app-view:inbox", "app-view:inbox"),
    ).toBe("session-1");
    expect(
      nextDisplayedTabAfterClose(reordered, "app-view:notes", "app-view:notes"),
    ).toBe("session-2");
  });

  it("does not change selection when an inactive tab is closed", () => {
    expect(
      nextDisplayedTabAfterClose(tabs, "session-2", "app-view:notes"),
    ).toBeUndefined();
  });

  it("has no neighbor for an absent or final tab", () => {
    expect(
      nextDisplayedTabAfterClose(tabs, "missing", "missing"),
    ).toBeUndefined();
    expect(
      nextDisplayedTabAfterClose([tabs[1]], tabs[1].id, tabs[1].id),
    ).toBeUndefined();
    expect(
      nextDisplayedTabAfterClose([], "missing", "missing"),
    ).toBeUndefined();
  });
});
