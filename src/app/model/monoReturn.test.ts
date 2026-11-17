import { describe, expect, it } from "vitest";
import { monoReturnFor, type MonoReturn } from "./monoReturn";

const back: MonoReturn<string> = {
  monoId: "mono-1",
  sessionId: "session-1",
  tabId: "tab-1",
  path: "/p/notes.md",
  view: "documents",
};

describe("monoReturnFor", () => {
  it("returns to the Mono when its file closes in the tab it opened in", () => {
    expect(
      monoReturnFor(back, { tabId: "tab-1", paths: ["/p/notes.md"] }, false),
    ).toBe(back);
  });

  it("ignores other files, other tabs, and no recorded return", () => {
    expect(
      monoReturnFor(back, { tabId: "tab-1", paths: ["/p/other.md"] }, false),
    ).toBeUndefined();
    expect(
      monoReturnFor(back, { tabId: "tab-2", paths: ["/p/notes.md"] }, false),
    ).toBeUndefined();
    expect(
      monoReturnFor(undefined, { tabId: "tab-1", paths: [] }, false),
    ).toBeUndefined();
  });

  it("leaves a close alone while a Mono already shows", () => {
    expect(
      monoReturnFor(back, { tabId: "tab-1", paths: ["/p/notes.md"] }, true),
    ).toBeUndefined();
  });
});
