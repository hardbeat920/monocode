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
} from "./paths";

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
