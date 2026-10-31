import { describe, expect, it } from "vitest";

import { mobileQuestionReply, mobileWaiting } from "./mobileWorkspace";
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

const prompt = {
  requestId: 1,
  questions: [
    {
      id: "colour",
      prompt: "Which colour?",
      multiSelect: false,
      allowCustom: true,
      options: [
        { id: "red", label: "Red" },
        { id: "blue", label: "Blue" },
      ],
    },
    {
      id: "size",
      prompt: "Which sizes?",
      multiSelect: true,
      allowCustom: false,
      options: [
        { id: "s", label: "Small" },
        { id: "l", label: "Large" },
      ],
    },
  ],
};

describe("mobile question replies", () => {
  it("keeps the questions and options asked, and a deliberate partial answer", () => {
    expect(
      mobileQuestionReply(
        { kind: "answered", answers: { colour: ["red"], size: ["s", "xl"], other: ["x"] } },
        prompt,
      ),
    ).toEqual({ kind: "answered", answers: { colour: ["red"], size: ["s"] } });
    expect(
      mobileQuestionReply(
        { kind: "answered", answers: { colour: ["__custom__"] }, custom: { colour: " Teal " } },
        prompt,
      ),
    ).toEqual({ kind: "answered", answers: { colour: ["__custom__"] }, custom: { colour: "Teal" } });
  });

  it("skips only when asked to, rejecting answers that leave nothing answered", () => {
    expect(mobileQuestionReply({ kind: "skipped" }, prompt)).toEqual({ kind: "skipped" });
    expect(mobileQuestionReply({ kind: "answered", answers: { colour: ["green"] } }, prompt)).toBeUndefined();
    expect(
      mobileQuestionReply(
        { kind: "answered", answers: { colour: ["__custom__"] }, custom: { colour: "x".repeat(10_001) } },
        prompt,
      ),
    ).toBeUndefined();
    expect(mobileQuestionReply({ kind: "maybe" }, prompt)).toBeUndefined();
    expect(mobileQuestionReply(null, prompt)).toBeUndefined();
  });
});
