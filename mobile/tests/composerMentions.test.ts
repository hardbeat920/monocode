import { expect, it } from "vitest";
import {
  TAG_ICON_SLOT,
  composerMentionParts,
  mentionIconPrefixes,
  prepareComposerMentions,
  updateComposerMentions,
  type ComposerMention,
} from "@/features/sessions/composerMentions";
function tag(
  text: string,
  label: string,
  path = label,
  start = text.indexOf(`@${label}`),
): ComposerMention {
  return {
    label,
    canonicalLabel: path,
    name: label,
    path,
    isDir: label === "host",
    start,
    end: start + 1 + label.length,
  };
}
it("recognizes short selected names while preserving exact text and UTF-16 offsets", () => {
  const text = "🔍 @host @main.tsx, then @main.rs";
  const files = [
    tag(text, "host"),
    tag(text, "main.tsx", "src/main.tsx"),
    tag(text, "main.rs", "src/main.rs"),
  ];
  const parts = composerMentionParts(text, files);
  expect(parts.map((part) => part.text).join("")).toBe(text);
  expect(
    parts
      .filter((part) => part.file)
      .map((part) => [part.text, part.start, part.file!.name]),
  ).toEqual([
    ["@host", 3, "host"],
    ["@main.tsx", 9, "main.tsx"],
    ["@main.rs", 25, "main.rs"],
  ]);
  expect(prepareComposerMentions(text, files)).toBe(
    "🔍 @host @src/main.tsx, then @src/main.rs",
  );
});
it("keeps duplicate basenames distinct when sending and after edits before/between tags", () => {
  const before = "@main.tsx and @main.tsx";
  const files = [
    tag(before, "main.tsx", "web/main.tsx"),
    tag(before, "main.tsx", "host/main.tsx", before.lastIndexOf("@")),
  ];
  expect(prepareComposerMentions(before, files)).toBe(
    "@web/main.tsx and @host/main.tsx",
  );
  const after = "Review @main.tsx and then @main.tsx";
  const prefixed = "Review " + before;
  const shifted = updateComposerMentions(
    prefixed,
    after,
    updateComposerMentions(before, prefixed, files),
  );
  expect(prepareComposerMentions(after, shifted)).toBe(
    "Review @web/main.tsx and then @host/main.tsx",
  );
});
it("untags references edited inside their filename without affecting another duplicate", () => {
  const before = "@main.tsx @main.tsx";
  const files = [
    tag(before, "main.tsx", "web/main.tsx"),
    tag(before, "main.tsx", "host/main.tsx", before.lastIndexOf("@")),
  ];
  const after = "@other.tsx @main.tsx";
  const updated = updateComposerMentions(before, after, files);
  expect(updated).toHaveLength(1);
  expect(prepareComposerMentions(after, updated)).toBe(
    "@other.tsx @host/main.tsx",
  );
});
it("supports a visible filename containing spaces without losing its canonical alias", () => {
  const text = "Review @read me.md please";
  const file = {
    ...tag(text, "read me.md", "docs/read me.md"),
    canonicalLabel: "docs/read-me.md~2",
  };
  expect(composerMentionParts(text, [file]).some((part) => part.file)).toBe(
    true,
  );
  expect(prepareComposerMentions(text, [file])).toBe(
    "Review @docs/read-me.md~2 please",
  );
  expect(prepareComposerMentions(text, [{ ...file, literalPath: true }])).toBe(
    "Review `docs/read me.md` please",
  );
});
it("does not style or send email, extended names, or quoted tokens as selected references", () => {
  for (const text of ["mail@host", "@host-edited", "> @host"]) {
    const file = tag(text, "host");
    expect(composerMentionParts(text, [file]).some((part) => part.file)).toBe(
      false,
    );
    expect(prepareComposerMentions(text, [file])).toBe(text);
  }
});
it("locates icons on native wrapped lines without changing text widths", () => {
  const text = "Review @host and\n@main.tsx now";
  const parts = composerMentionParts(text, [
    tag(text, "host"),
    tag(text, "main.tsx", "src/main.tsx"),
  ]);
  const markers = mentionIconPrefixes(text, parts, [
    { text: "Review @host and\n", x: 0, y: 0, height: 22 },
    { text: "@main.tsx now", x: 0, y: 22, height: 22 },
  ]);
  expect(markers.map(({ prefix, y, file }) => [prefix, y, file.name])).toEqual([
    ["Review @", 0, "host"],
    ["@", 22, "main.tsx"],
  ]);
});

it("preserves the second duplicate's identity when the first tag is deleted", () => {
  const before = "@main.tsx @main.tsx";
  const files = [
    tag(before, "main.tsx", "web/main.tsx"),
    tag(before, "main.tsx", "host/main.tsx", before.lastIndexOf("@")),
  ];
  const after = "@main.tsx";
  const updated = updateComposerMentions(before, after, files, {
    start: 0,
    end: 10,
  });
  expect(prepareComposerMentions(after, updated)).toBe("@host/main.tsx");
});

it("uses a blank native icon slot, with no @ glyph, while sending the canonical reference", () => {
  const text = `Review ${TAG_ICON_SLOT}main.tsx`;
  const file = {
    ...tag(text, "main.tsx", "src/main.tsx", text.indexOf(TAG_ICON_SLOT)),
  };
  expect(text).not.toContain("@");
  expect(
    composerMentionParts(text, [file]).find((part) => part.file)?.text,
  ).toBe(`${TAG_ICON_SLOT}main.tsx`);
  expect(prepareComposerMentions(text, [file])).toBe("Review @src/main.tsx");
});
