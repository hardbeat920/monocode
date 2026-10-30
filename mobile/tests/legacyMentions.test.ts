import { expect, it } from "vitest";
import { prepareLiteralMentions } from "@/features/sessions/legacyMentions";
it("sends selected legacy spaced paths literally without guessing conflicting desktop aliases", () => {
  const refs = [
    { label: "docs/read%20me.md", path: "docs/read me.md", isDir: false },
    { label: "src/my%20folder", path: "src/my folder", isDir: true },
  ];
  expect(
    prepareLiteralMentions(
      "Review @docs/read%20me.md, @src/my%20folder and @docs/read-me.md",
      refs,
    ),
  ).toBe("Review `docs/read me.md`, `src/my folder/` and @docs/read-me.md");
});
it("leaves unselected text, email addresses, edited tokens and quoted mentions alone", () => {
  const text =
    "person@docs/read%20me.md @docs/read%20me.md-edited\n> @docs/read%20me.md\n@normal";
  expect(
    prepareLiteralMentions(text, [
      { label: "docs/read%20me.md", path: "docs/read me.md", isDir: false },
    ]),
  ).toBe(text);
});
