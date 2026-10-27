import { describe, expect, it } from "vitest";

import { mobileWaiting } from "./mobileWorkspace";
import { browserState } from "../../settings/model/browser.fixtures";
import type { Block } from "../../sessions/model/session";

const asking = browserState({
  sessions: [
    {
      session_id: "s1",
      window: "main",
      granted: false,
      turn_active: true,
      tabs: 0,
      requested: true,
      turned_off: false,
    },
  ],
});
const approval = {
  id: "b1",
  role: "tool",
  text: "",
  approval: { decided: false },
} as unknown as Block;

describe("what a session waits on, as the phone sees it", () => {
  it("flags a browser request for the phone to answer, after approvals and questions", () => {
    const session = { id: "s1", blocks: [] };
    expect(mobileWaiting(session, null)).toEqual({});
    expect(mobileWaiting(session, asking)).toEqual({
      waiting: "browser",
      browserRequest: true,
    });
    expect(mobileWaiting({ ...session, blocks: [approval] }, asking)).toEqual({
      waiting: "approval",
      browserRequest: true,
    });
    expect(mobileWaiting({ id: "s2", blocks: [] }, asking)).toEqual({});
    expect(mobileWaiting(session, { ...asking, toggle_on: false })).toEqual({});
  });
});
