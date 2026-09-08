import { describe, expect, it } from "vitest";
import { parseSessionStoreChanged } from "./sessionStore";

describe("session-store-changed payload", () => {
  it("parses upserted summaries", () => {
    const parsed = parseSessionStoreChanged({
      kind: "upserted",
      summary: { id: "s1", cwd: "/proj", harness: "claude", title: "Hi" },
    });
    expect(parsed).toEqual({
      kind: "upserted",
      summary: { id: "s1", cwd: "/proj", harness: "claude", title: "Hi" },
    });
  });

  it("parses deleted / archived / pinned and rejects junk", () => {
    expect(
      parseSessionStoreChanged({
        kind: "deleted",
        sessionId: "s1",
        cwd: "/proj",
      }),
    ).toEqual({ kind: "deleted", sessionId: "s1", cwd: "/proj" });
    expect(
      parseSessionStoreChanged({
        kind: "archived",
        sessionId: "s1",
        archived: true,
      }),
    ).toEqual({ kind: "archived", sessionId: "s1", archived: true });
    expect(
      parseSessionStoreChanged({
        kind: "pinned",
        sessionId: "s1",
        pinned: false,
      }),
    ).toEqual({ kind: "pinned", sessionId: "s1", pinned: false });
    expect(parseSessionStoreChanged({ kind: "upserted" })).toBeNull();
    expect(parseSessionStoreChanged({ kind: "deleted" })).toBeNull();
    expect(parseSessionStoreChanged(null)).toBeNull();
  });
});
