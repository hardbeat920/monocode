import { describe, expect, it } from "vitest";
import { editSessionDraft, newSession, removeSessionDraft } from "./session";

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

describe("editSessionDraft", () => {
  it("rewrites the draft text and keeps its identity and attachments", () => {
    const session = newSession("codex", "/repo");
    const attachment = {
      id: "a",
      name: "notes.md",
      mimeType: "text/markdown",
      kind: "file" as const,
      size: 4,
    };
    session.blocks = [
      { id: "sent", role: "user", text: "Start here" },
      {
        id: "draft",
        role: "user",
        text: "Maybe later",
        draft: true,
        attachments: [attachment],
        appRequestId: "request",
      },
    ];

    const updated = editSessionDraft(session, "draft", "Do it now");

    expect(updated?.blocks[0]).toBe(session.blocks[0]);
    expect(updated?.blocks[1]).toEqual({
      ...session.blocks[1],
      text: "Do it now",
    });
  });

  it("retitles a session named after the draft but keeps a custom title", () => {
    const session = newSession("codex", "/repo");
    session.title = "codex · Maybe later";
    session.blocks = [
      { id: "draft", role: "user", text: "Maybe later", draft: true },
    ];

    expect(editSessionDraft(session, "draft", "Do it now")?.title).toBe(
      "codex · Do it now",
    );

    session.title = "codex · Keep this name";
    expect(editSessionDraft(session, "draft", "Do it now")?.title).toBe(
      "codex · Keep this name",
    );
  });

  it("rejects empty text, sent messages, and unknown blocks", () => {
    const session = newSession("codex", "/repo");
    session.blocks = [
      { id: "sent", role: "user", text: "Keep this" },
      { id: "draft", role: "user", text: "Maybe later", draft: true },
    ];

    expect(editSessionDraft(session, "draft", "  ")).toBeUndefined();
    expect(editSessionDraft(session, "sent", "Changed")).toBeUndefined();
    expect(editSessionDraft(session, "missing", "Changed")).toBeUndefined();
  });
});
