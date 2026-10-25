// @vitest-environment happy-dom
import { afterEach, describe, expect, it } from "vitest";
import {
  browserState,
  browserTab,
} from "../../settings/model/browser.fixtures";
import {
  loadBrowserPanePlacement,
  newlyOpenBrowserTabs,
  saveBrowserPanePlacement,
} from "./browserPane";

describe("browser panes", () => {
  afterEach(() => localStorage.clear());

  it("count a tab as new once it is open and was not before", () => {
    const attaching = browserTab({ tab: "t1", state: "attaching" });
    const open = browserTab({ tab: "t1", state: "open" });
    const other = browserTab({ tab: "t2", state: "open" });
    expect(
      newlyOpenBrowserTabs(
        browserState({ tabs: [] }),
        browserState({ tabs: [attaching] }),
      ),
    ).toEqual([]);
    expect(
      newlyOpenBrowserTabs(
        browserState({ tabs: [attaching] }),
        browserState({ tabs: [open] }),
      ),
    ).toEqual([open]);
    expect(
      newlyOpenBrowserTabs(
        browserState({ tabs: [open] }),
        browserState({ tabs: [open, other] }),
      ),
    ).toEqual([other]);
  });

  it("remember where the pane was left, ignoring anything malformed", () => {
    expect(loadBrowserPanePlacement()).toBeNull();
    saveBrowserPanePlacement({ edge: "left", size: 0.4 });
    expect(loadBrowserPanePlacement()).toEqual({ edge: "left", size: 0.4 });
    localStorage.setItem(
      "monocode.browserPanePlacement",
      JSON.stringify({ edge: "middle", size: 2 }),
    );
    expect(loadBrowserPanePlacement()).toBeNull();
  });
});
