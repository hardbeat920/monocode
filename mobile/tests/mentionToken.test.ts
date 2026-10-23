import { expect, it } from "vitest";
import {
  mentionTokenAt,
  replaceMentionToken,
} from "@/features/sessions/mentionToken";
import tokenFixtures from "../../src/features/files/model/fileMentionTokens.fixture.json";

// Shared data fixtures keep desktop/mobile behavior aligned without loading
// either application's dependency graph into the other application's tests.
it.each(tokenFixtures)(
  "matches desktop token behavior: $text at $cursor",
  ({ text, cursor, token: expected, replacement }) => {
    const token = mentionTokenAt(text, cursor);
    expect(token).toEqual(expected);
    if (token)
      expect(replaceMentionToken(text, token, "docs/read-me.md")).toBe(
        replacement,
      );
  },
);
it("limits queries and does not change text outside the active mention", () => {
  expect(mentionTokenAt("@" + "a".repeat(121), 122)).toBeNull();
  const text = "@one and @partial after";
  expect(
    replaceMentionToken(text, mentionTokenAt(text, 14)!, "folder/file.ts"),
  ).toBe("@one and @folder/file.ts after");
});
