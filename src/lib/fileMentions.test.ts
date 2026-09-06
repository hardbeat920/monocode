import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ProjectFile } from "./fs";
import { loadProjectFiles } from "./fileIndex";
import {
  applyFileMentionsToTurn,
  buildMentionIndex,
  fileMentionParts,
  fileMentionsInText,
  mentionLabel,
  mentionTokenAt,
  rankMentionFiles,
  replaceMentionToken,
  withMentionDirectories,
} from "./fileMentions";

vi.mock("./fileIndex", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./fileIndex")>();
  return {
    ...actual,
    loadProjectFiles: vi.fn(actual.loadProjectFiles),
  };
});

const list = vi.mocked(loadProjectFiles);

const files: ProjectFile[] = [
  {
    name: "App.tsx",
    path: "/p/apps/desktop/src/App.tsx",
    relative: "apps/desktop/src/App.tsx",
  },
  {
    name: "App.tsx",
    path: "/p/apps/web/src/App.tsx",
    relative: "apps/web/src/App.tsx",
  },
  {
    name: "Composer.tsx",
    path: "/p/src/chrome/Composer.tsx",
    relative: "src/chrome/Composer.tsx",
  },
  {
    name: "read me.md",
    path: "/p/docs/read me.md",
    relative: "docs/read me.md",
  },
];

const index = buildMentionIndex(files);

describe("mentionTokenAt", () => {
  it("reads the @token the cursor is in", () => {
    expect(mentionTokenAt("@Comp", 5)).toEqual({
      start: 0,
      end: 5,
      query: "Comp",
    });
    expect(mentionTokenAt("look at @src/App", 16)).toEqual({
      start: 8,
      end: 16,
      query: "src/App",
    });
  });

  it("ignores emails and mid-word @", () => {
    expect(mentionTokenAt("nick@example.com", 8)).toBeNull();
    expect(mentionTokenAt("a@b", 3)).toBeNull();
  });

  it("closes after a space", () => {
    expect(mentionTokenAt("@App.tsx now", 12)).toBeNull();
  });
});

describe("replaceMentionToken", () => {
  it("inserts @label and a trailing space", () => {
    expect(
      replaceMentionToken("@Comp", { start: 0, end: 5, query: "Comp" }, "Composer.tsx"),
    ).toBe("@Composer.tsx ");
    expect(
      replaceMentionToken("x @a y", { start: 2, end: 4, query: "a" }, "App.tsx"),
    ).toBe("x @App.tsx y");
  });
});

describe("buildMentionIndex", () => {
  it("labels unique basenames short and ambiguous ones by path", () => {
    expect(mentionLabel(files[2], index)).toBe("Composer.tsx");
    expect(mentionLabel(files[0], index)).toBe("apps/desktop/src/App.tsx");
  });

  it("keeps a spaced path as a single @ token", () => {
    expect(mentionLabel(files[3], index)).toBe("docs/read-me.md");
    expect(index.labels.get("docs/read-me.md")).toBe(files[3]);
    expect(index.labels.has("read me.md")).toBe(false);
  });

  it("does not steal a token already owned by a real path", () => {
    const spaced: ProjectFile = {
      name: "read me.md",
      path: "/p/notes/read me.md",
      relative: "notes/read me.md",
    };
    const existing: ProjectFile = {
      name: "read-me.md",
      path: "/p/notes/read-me.md",
      relative: "notes/read-me.md",
    };
    const mixed = buildMentionIndex([spaced, existing]);
    expect(mentionLabel(existing, mixed)).toBe("read-me.md");
    expect(mixed.labels.get("notes/read-me.md")).toBe(existing);
    expect(mentionLabel(spaced, mixed)).toBe("notes/read-me.md~2");
    expect(mixed.labels.get("notes/read-me.md~2")).toBe(spaced);
  });

  it("ignores paths that leave the project or break the tokenizer", () => {
    const unsafe = buildMentionIndex([
      { name: "secret", path: "/etc/secret", relative: "../secret" },
      { name: "abs", path: "/tmp/abs", relative: "/tmp/abs" },
      { name: "at.md", path: "/p/at.md", relative: "see@me.md" },
      { name: "nul", path: "/p/nul", relative: "bad\0name.md" },
      { name: "ls.md", path: "/p/ls.md", relative: "docs/read\u2028me.md" },
      { name: "ps.md", path: "/p/ps.md", relative: "docs/read\u2029me.md" },
      { name: "nel.md", path: "/p/nel.md", relative: "a\u0085b.md" },
      { name: "bidi.md", path: "/p/bidi.md", relative: "photo\u202Egpj.md" },
      { name: "zwsp.ts", path: "/p/zwsp.ts", relative: "file\u200B.ts" },
    ]);
    expect(unsafe.labels.size).toBe(0);
  });

  it("always accepts the relative path as a label", () => {
    expect(index.labels.get("apps/web/src/App.tsx")).toBe(files[1]);
    expect(index.labels.get("src/chrome/Composer.tsx")).toBe(files[2]);
  });

  it("indexes parent folders so they can be mentioned", () => {
    const chrome = index.labels.get("src/chrome");
    expect(chrome).toMatchObject({
      name: "chrome",
      relative: "src/chrome",
      path: "/p/src/chrome",
      isDir: true,
    });
    expect(mentionLabel(chrome!, index)).toBe("src/chrome");
    expect(index.labels.has("chrome")).toBe(false);

    const src = index.labels.get("src");
    expect(src).toMatchObject({
      relative: "src",
      path: "/p/src",
      isDir: true,
    });
    expect(index.labels.get("apps/web/src")).toMatchObject({
      relative: "apps/web/src",
      isDir: true,
    });
  });

  it("still mentions a folder when its only file has a space in the name", () => {
    expect(index.labels.get("docs")).toMatchObject({
      relative: "docs",
      path: "/p/docs",
      isDir: true,
    });
  });

  it("inserts a spaced folder as a normal @ token", () => {
    const cat: ProjectFile = {
      name: "cat.png",
      path: "/p/My Photos/cat.png",
      relative: "My Photos/cat.png",
    };
    const photos = buildMentionIndex([cat]);
    const folder = [...photos.labels.values()].find(
      (file) => file.relative === "My Photos",
    );
    expect(folder).toMatchObject({ isDir: true, relative: "My Photos" });
    expect(mentionLabel(folder!, photos)).toBe("My-Photos");
    expect(mentionLabel(cat, photos)).toBe("cat.png");
  });
});

describe("fileMentionParts", () => {
  it("splits known mentions out of the surrounding text", () => {
    expect(fileMentionParts("fix @Composer.tsx now", index.labels)).toEqual([
      { text: "fix " },
      { text: "@Composer.tsx", file: files[2] },
      { text: " now" },
    ]);
  });

  it("leaves unknown mentions and emails alone", () => {
    expect(fileMentionParts("ping @nobody.tsx", index.labels)).toEqual([
      { text: "ping @nobody.tsx" },
    ]);
    expect(fileMentionParts("nick@example.com", index.labels)).toEqual([
      { text: "nick@example.com" },
    ]);
  });

  it("keeps trailing punctuation outside the mention", () => {
    expect(fileMentionParts("see @Composer.tsx, then", index.labels)).toEqual([
      { text: "see " },
      { text: "@Composer.tsx", file: files[2] },
      { text: ", then" },
    ]);
  });

  it("highlights folder mentions, including a trailing slash", () => {
    const chrome = index.labels.get("src/chrome");
    expect(fileMentionParts("look in @src/chrome please", index.labels)).toEqual([
      { text: "look in " },
      { text: "@src/chrome", file: chrome },
      { text: " please" },
    ]);
    expect(fileMentionParts("look in @src/ next", index.labels)).toEqual([
      { text: "look in " },
      { text: "@src", file: index.labels.get("src") },
      { text: "/ next" },
    ]);
  });

  it("ignores file mentions inside Markdown blockquotes", () => {
    const text = "@Composer.tsx\n> @apps/web/src/App.tsx";
    expect(mentionTokenAt(text, text.indexOf("@apps/web") + 4)).toBeNull();
    expect(
      fileMentionsInText(text, index.labels).map((hit) => hit.label),
    ).toEqual(["Composer.tsx"]);
    expect(
      fileMentionParts(text, index.labels).filter((part) => part.file),
    ).toHaveLength(1);
  });
});

describe("fileMentionsInText", () => {
  it("highlights the encoded token for a spaced path", () => {
    expect(fileMentionParts("see @docs/read-me.md now", index.labels)).toEqual([
      { text: "see " },
      { text: "@docs/read-me.md", file: files[3] },
      { text: " now" },
    ]);
  });

  it("collects each referenced file once", () => {
    const hits = fileMentionsInText(
      "@Composer.tsx and @apps/web/src/App.tsx and @Composer.tsx",
      index.labels,
    );
    expect(hits.map((hit) => hit.label)).toEqual([
      "Composer.tsx",
      "apps/web/src/App.tsx",
    ]);
  });
});

describe("rankMentionFiles", () => {
  it("offers recents first when nothing is typed", () => {
    const ranked = rankMentionFiles(files, "", [files[1].path]);
    expect(ranked[0].path).toBe(files[1].path);
  });

  it("fuzzy matches spaced names from a space-free query", () => {
    const ranked = rankMentionFiles(files, "read", []);
    expect(ranked[0].path).toBe(files[3].path);
    expect(rankMentionFiles(files, "compo", [])[0].path).toBe(files[2].path);
  });

  it("lists a folder whose name has spaces", () => {
    const photos = [
      {
        name: "cat.png",
        path: "/p/My Photos/cat.png",
        relative: "My Photos/cat.png",
      },
    ];
    expect(
      rankMentionFiles(photos, "photos", []).some(
        (file) => file.relative === "My Photos" && file.isDir,
      ),
    ).toBe(true);
    expect(rankMentionFiles(photos, "cat", [])[0].path).toBe(photos[0].path);
  });

  it("offers folders alongside files", () => {
    const ranked = rankMentionFiles(files, "chrome", []);
    expect(ranked[0]).toMatchObject({
      relative: "src/chrome",
      isDir: true,
    });
    expect(rankMentionFiles(files, "src/", [])[0]).toMatchObject({
      relative: "src",
      isDir: true,
    });
  });

  it("puts folders first when nothing is typed", () => {
    const ranked = rankMentionFiles(files, "", []);
    expect(ranked.filter((file) => file.isDir).map((file) => file.relative)).toEqual(
      expect.arrayContaining(["apps", "docs", "src", "src/chrome"]),
    );
    expect(ranked[0].isDir).toBe(true);
  });
});

describe("withMentionDirectories", () => {
  it("derives unique parent folders from file paths", () => {
    const entries = withMentionDirectories(files);
    const dirs = entries.filter((file) => file.isDir);
    expect(dirs).toEqual(
      expect.arrayContaining([
        {
          name: "src",
          path: "/p/src",
          relative: "src",
          isDir: true,
        },
        {
          name: "chrome",
          path: "/p/src/chrome",
          relative: "src/chrome",
          isDir: true,
        },
        {
          name: "apps",
          path: "/p/apps",
          relative: "apps",
          isDir: true,
        },
      ]),
    );
    expect(withMentionDirectories(entries)).toEqual(entries);
  });

  it("keeps parent folders that contain spaces", () => {
    const entries = withMentionDirectories([
      {
        name: "cat.png",
        path: "/p/My Photos/cat.png",
        relative: "My Photos/cat.png",
      },
    ]);
    expect(entries.some((file) => file.relative === "My Photos" && file.isDir)).toBe(
      true,
    );
  });

  it("rebuilds folder paths on Windows separators", () => {
    const [dir] = withMentionDirectories([
      {
        name: "App.tsx",
        path: "C:\\p\\apps\\web\\src\\App.tsx",
        relative: "apps/web/src/App.tsx",
      },
    ]).filter((file) => file.relative === "apps/web");
    expect(dir).toMatchObject({
      name: "web",
      path: "C:\\p\\apps\\web",
      isDir: true,
    });
  });
});

describe("applyFileMentionsToTurn", () => {
  beforeEach(() => {
    list.mockReset();
    list.mockResolvedValue(files);
  });

  it("maps a spaced-file token to the real relative path on send", async () => {
    const out = await applyFileMentionsToTurn("look at @docs/read-me.md", "/p");
    expect(out).toContain("look at @docs/read-me.md");
    expect(out).toContain("- @docs/read-me.md → docs/read me.md");
  });

  it("does not invent a path from a token that is not in the index", async () => {
    const out = await applyFileMentionsToTurn(
      "look at @notes/read-me.md~2",
      "/p",
    );
    expect(out).toBe("look at @notes/read-me.md~2");
  });
});
