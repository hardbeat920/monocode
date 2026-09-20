import { describe, expect, it } from "vitest";
import {
  canTabVisitBack,
  canTabVisitForward,
  emptyTabVisitHistory,
  pruneTabVisitHistory,
  recordTabVisit,
  tabVisitBack,
  tabVisitForward,
} from "./tabVisitHistory";

describe("tab visit history", () => {
  it("returns to the previous tab", () => {
    let history = recordTabVisit(emptyTabVisitHistory("a"), "b");
    expect(canTabVisitBack(history)).toBe(true);
    expect(canTabVisitForward(history)).toBe(false);

    history = tabVisitBack(history)!;
    expect(history.current).toBe("a");
    expect(canTabVisitBack(history)).toBe(false);
    expect(canTabVisitForward(history)).toBe(true);
  });

  it("restores the tab after going back", () => {
    let history = recordTabVisit(emptyTabVisitHistory("a"), "b");
    history = tabVisitBack(history)!;
    history = tabVisitForward(history)!;
    expect(history.current).toBe("b");
    expect(canTabVisitForward(history)).toBe(false);
  });

  it("drops the forward stack when visiting a new tab after back", () => {
    let history = recordTabVisit(emptyTabVisitHistory("a"), "b");
    history = tabVisitBack(history)!;
    history = recordTabVisit(history, "c");
    expect(history.current).toBe("c");
    expect(canTabVisitForward(history)).toBe(false);
    expect(tabVisitBack(history)?.current).toBe("a");
  });

  it("ignores recording the tab already current", () => {
    const start = emptyTabVisitHistory("a");
    expect(recordTabVisit(start, "a")).toBe(start);
  });

  it("does not keep a closed current tab on the back stack", () => {
    const history = pruneTabVisitHistory(
      recordTabVisit(emptyTabVisitHistory("a"), "b"),
      new Set(["a"]),
      "a",
    );
    expect(history).toEqual({ back: [], forward: [], current: "a" });
    expect(canTabVisitBack(history)).toBe(false);
  });

  it("skips a closed tab in the middle of the stack", () => {
    let history = recordTabVisit(emptyTabVisitHistory("a"), "b");
    history = recordTabVisit(history, "c");
    history = pruneTabVisitHistory(history, new Set(["a", "c"]), "c");
    expect(tabVisitBack(history)?.current).toBe("a");
  });

  it("collapses a round-trip once the other tab closes", () => {
    let history = recordTabVisit(emptyTabVisitHistory("a"), "b");
    history = recordTabVisit(history, "a");
    history = pruneTabVisitHistory(history, new Set(["a"]), "a");
    expect(history).toEqual({ back: [], forward: [], current: "a" });
  });
});
