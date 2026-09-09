import { describe, expect, it } from "vitest";
import {
  displayPath,
  isEqualOrInside,
  joinPath,
  parentPath,
  prettyCwd,
  projectName,
  rebasePath,
  slash,
  resolveWorkspaceFileReference,
  resolveWorkspacePath,
} from "./paths";

describe("workspace file references", () => {
  it.each([
    ["src/main.ts:12:3", "/repo", "/repo/src/main.ts", { line: 12, column: 3 }],
    [
      "./docs/My%20Guide.md#L7-L9",
      "/repo",
      "/repo/docs/My Guide.md",
      { line: 7 },
    ],
    ["C:\\repo\\main.ts:4", "C:/repo", "C:/repo/main.ts", { line: 4 }],
    [
      "file:///C:/My%20Project/main.ts#L2",
      "C:/repo",
      "C:/My Project/main.ts",
      { line: 2 },
    ],
    ["file://localhost/repo/main.ts:2", "/repo", "/repo/main.ts", { line: 2 }],
    ["report%23L2.md", "/repo", "/repo/report#L2.md", undefined],
    ["docs/guide.md#installation", "/repo", "/repo/docs/guide.md", undefined],
    ["docs/guide.md#installation:12", "/repo", "/repo/docs/guide.md", undefined],
    ["report%23L2.md#installation", "/repo", "/repo/report#L2.md", undefined],
    ["progress%done.md", "/repo", "/repo/progress%done.md", undefined],
    ["Dockerfile", "/repo", "/repo/Dockerfile", undefined],
    ["src/main.ts:0", "/repo", "/repo/src/main.ts", undefined],
  ] as const)(
    "resolves %s with its source location",
    (href, cwd, path, navigation) => {
      expect(resolveWorkspaceFileReference(href, cwd)).toEqual({
        path,
        navigation,
      });
    },
  );

  it.each([
    "https://example.com/src/main.ts",
    "//example.com/main.ts",
    "javascript:../main.ts",
    "javascript%3A../main.ts",
    "data:text/html,file.md",
    "mailto:readme@example.com",
    "#section",
  ])("does not turn %s into a local file", (href) => {
    expect(resolveWorkspaceFileReference(href, "/repo")).toBeUndefined();
  });

  it("does not decode a literal percent sequence again in an already-resolved filesystem path", () => {
    const file = resolveWorkspaceFileReference(
      "/repo/progress%2520.md",
      "/repo",
    )!;
    expect(file.path).toBe("/repo/progress%20.md");
    expect(resolveWorkspacePath(file.path, "/repo")).toBe(file.path);
  });
});

describe("slash", () => {
  it("preserves backslashes in absolute Unix filenames", () => {
    expect(slash("/tmp/a\\b.txt")).toBe("/tmp/a\\b.txt");
    expect(joinPath("/tmp", "a\\b.txt")).toBe("/tmp/a\\b.txt");
    expect(parentPath("/tmp/a\\b.txt")).toBe("/tmp");
  });
  it("normalizes Windows separators", () => {
    expect(slash("C:\\Users\\me\\code")).toBe("C:/Users/me/code");
  });
});

describe("prettyCwd", () => {
  it("collapses unix and Windows home prefixes", () => {
    expect(prettyCwd("/Users/me")).toBe("~");
    expect(prettyCwd("/Users/me/code")).toBe("~/code");
    expect(prettyCwd("C:\\Users\\me")).toBe("~");
    expect(prettyCwd("C:/Users/me/code/app")).toBe("~/code/app");
  });
});

describe("parentPath and joinPath", () => {
  it("preserves Windows filesystem roots", () => {
    expect(parentPath("//server/share")).toBe("//server/share");
    expect(rebasePath("C:/old", "C:/old", "D:/")).toBe("D:/");
  });
  it("walks Windows drive paths", () => {
    expect(parentPath("C:/Users/me/code")).toBe("C:/Users/me");
    expect(parentPath("C:/Users")).toBe("C:/");
    expect(parentPath("C:/")).toBe("C:/");
    expect(joinPath("C:/Users/me", "code/app")).toBe("C:/Users/me/code/app");
    expect(joinPath("C:/Users/me/code", "..")).toBe("C:/Users/me");
    expect(joinPath("C:/", "Users")).toBe("C:/Users");
  });
});

describe("path relations", () => {
  it("treats backslash and slash as the same path", () => {
    expect(isEqualOrInside("C:\\Users\\me\\app\\src", "C:/Users/me/app")).toBe(
      true,
    );
    expect(isEqualOrInside("C:/Users/me", "C:/")).toBe(true);
    expect(rebasePath("C:\\Users\\me\\app\\src\\a.ts", "C:/Users/me/app", "D:/x")).toBe(
      "D:/x/src/a.ts",
    );
    expect(displayPath("C:\\Users\\me\\app\\src\\a.ts", "C:/Users/me/app")).toBe(
      "src/a.ts",
    );
    expect(projectName("C:\\Users\\me\\app")).toBe("app");
  });

  it("compares Windows paths without case", () => {
    expect(isEqualOrInside("c:/USERS/me/App/src", "C:/Users/ME/app")).toBe(true);
    expect(rebasePath("c:/USERS/me/App/src/a.ts", "C:/Users/ME/app", "D:/x")).toBe(
      "D:/x/src/a.ts",
    );
    expect(displayPath("c:/USERS/me/App/src/a.ts", "C:/Users/ME/app")).toBe(
      "src/a.ts",
    );
  });
});
