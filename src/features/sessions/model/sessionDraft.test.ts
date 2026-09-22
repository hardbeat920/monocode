import { describe, expect, it } from "vitest";
import { newSession, removeSessionDraft } from "./session";

describe("removeSessionDraft", () => {
  it("removes a follow-up draft without changing earlier conversation history", () => {
    const session = newSession("codex", "/repo");
    session.title = "codex · Existing thread";
    session.blocks = [
      { id: "sent", role: "user", text: "Start here" },
      { id: "reply", role: "assistant", text: "Done" },
      { id: "draft", role: "user", text: "Maybe later", draft: true },
    ];

    const updated = removeSessionDraft(session, "draft");

    expect(updated?.blocks).toEqual(session.blocks.slice(0, 2));
    expect(updated?.title).toBe("codex · Existing thread");
  });

  it("restores a draft-only session to a blank untitled state", () => {
    const session = newSession("codex", "/repo");
    session.title = "codex · Maybe later";
    session.blocks = [
      { id: "draft", role: "user", text: "Maybe later", draft: true },
    ];

    expect(removeSessionDraft(session, "draft")).toMatchObject({
      title: "codex",
      blocks: [],
    });
  });

  it("keeps a custom title when removing the only draft", () => {
    const session = newSession("codex", "/repo");
    session.title = "codex · Keep this name";
    session.blocks = [
      { id: "draft", role: "user", text: "Maybe later", draft: true },
    ];

    expect(removeSessionDraft(session, "draft")?.title).toBe(
      "codex · Keep this name",
    );
  });

  it("ignores sent messages and unknown blocks", () => {
    const session = newSession("codex", "/repo");
    session.blocks = [{ id: "sent", role: "user", text: "Keep this" }];

    expect(removeSessionDraft(session, "sent")).toBeUndefined();
    expect(removeSessionDraft(session, "missing")).toBeUndefined();
  });
});
