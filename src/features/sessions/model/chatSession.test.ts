import { afterEach, describe, expect, it } from "vitest";
import {
  CHATS_CWD,
  chatWorkCwd,
  isChatCwd,
  isChatSession,
  setChatWorkspaceDir,
} from "./chatSession";

afterEach(() => setChatWorkspaceDir(null));

describe("chat sessions", () => {
  it("treats only the home placeholder as a chat", () => {
    expect(isChatCwd(CHATS_CWD)).toBe(true);
    expect(isChatCwd("/Users/me/app")).toBe(false);
    expect(isChatCwd("")).toBe(false);
    expect(isChatCwd(undefined)).toBe(false);
    expect(isChatSession({ cwd: "~" })).toBe(true);
    expect(isChatSession({ cwd: "/Users/me/app" })).toBe(false);
    expect(isChatSession(undefined)).toBe(false);
  });

  it("keeps the home directory until the chats folder is known", () => {
    expect(chatWorkCwd("~")).toBe("~");
  });

  it("runs chats in the chats folder and leaves projects alone", () => {
    setChatWorkspaceDir("/data/monocode/chats");
    expect(chatWorkCwd("~")).toBe("/data/monocode/chats");
    expect(chatWorkCwd("/Users/me/app")).toBe("/Users/me/app");
  });

  it("ignores an unusable chats folder", () => {
    setChatWorkspaceDir("  ");
    expect(chatWorkCwd("~")).toBe("~");
    setChatWorkspaceDir("~");
    expect(chatWorkCwd("~")).toBe("~");
  });
});
