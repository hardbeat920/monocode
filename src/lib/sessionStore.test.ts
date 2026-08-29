import { describe, expect, it } from "vitest";
import { newSession } from "./session";
import { isPersistableId, sanitizeSessionForPersist } from "./sessionStore";

describe("isPersistableId", () => {
  it("accepts alphanumeric ids with hyphens and underscores", () => {
    expect(isPersistableId("acp-session-1")).toBe(true);
    expect(isPersistableId("abc_123")).toBe(true);
  });

  it("rejects filesystem paths", () => {
    expect(isPersistableId("/Users/me/.pi/agent/sessions/abc.jsonl")).toBe(
      false,
    );
  });
});

describe("sanitizeSessionForPersist", () => {
  it("omits a path-like provider session id so upsert can still snapshot git", () => {
    const session = newSession("pi", "/tmp/project");
    session.providerSessionId = "/Users/me/.pi/agent/sessions/abc.jsonl";
    session.blocks = [{ id: "u1", role: "user", text: "hey" }];

    expect(
      sanitizeSessionForPersist(session).providerSessionId,
    ).toBeUndefined();
  });

  it("keeps a UUID provider session id", () => {
    const session = newSession("pi", "/tmp/project");
    session.providerSessionId = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";
    session.blocks = [{ id: "u1", role: "user", text: "hey" }];

    expect(sanitizeSessionForPersist(session).providerSessionId).toBe(
      "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
    );
  });

  it("keeps a handoff divider and settles a preparing one", () => {
    const session = newSession("cursor", "/tmp/project");
    session.blocks = [
      { id: "u1", role: "user", text: "hey" },
      {
        id: "h1",
        role: "handoff",
        text: "",
        handoff: { from: "cursor", to: "claude", status: "preparing" },
      },
    ];
    const persisted = sanitizeSessionForPersist(session);
    expect(persisted.blocks[1]).toMatchObject({
      role: "handoff",
      handoff: { from: "cursor", to: "claude", status: "ready", pending: true },
    });
  });

  it("keeps a second-opinion card on the user turn", () => {
    const session = newSession("codex", "/tmp/project");
    session.blocks = [
      {
        id: "u1",
        role: "user",
        text: "Second opinion",
        secondOpinion: {
          from: "claude",
          to: "codex",
          request: "fix the footer",
          files: 2,
        },
      },
    ];
    expect(sanitizeSessionForPersist(session).blocks[0]).toMatchObject({
      role: "user",
      text: "Second opinion",
      secondOpinion: {
        from: "claude",
        to: "codex",
        request: "fix the footer",
        files: 2,
      },
    });
  });
});
