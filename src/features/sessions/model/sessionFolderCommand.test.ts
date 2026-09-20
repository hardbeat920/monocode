import { describe, expect, it } from "vitest";
import {
  consumeSessionFolderCommand,
  isSessionFolderCommand,
  runsSessionFolderCommandOnSpace,
} from "./sessionFolderCommand";

describe("session folder command", () => {
  it("matches a standalone /add-to-folder command", () => {
    expect(isSessionFolderCommand("/add-to-folder")).toBe(true);
    expect(isSessionFolderCommand("  /ADD-TO-FOLDER\n")).toBe(true);
  });

  it("does not consume ordinary prompt text", () => {
    expect(isSessionFolderCommand("/add-to-folder project")).toBe(false);
    expect(isSessionFolderCommand("mention /add-to-folder in docs")).toBe(
      false,
    );
    expect(isSessionFolderCommand("/add-to-folder-later")).toBe(false);
  });

  it("removes the leading command and keeps the agent prompt", () => {
    expect(
      consumeSessionFolderCommand("/add-to-folder   Build the settings screen"),
    ).toEqual({
      text: "Build the settings screen",
      matched: true,
    });
    expect(consumeSessionFolderCommand("Explain /add-to-folder")).toEqual({
      text: "Explain /add-to-folder",
      matched: false,
    });
  });

  it("runs on an unmodified space at the end of /add-to-folder", () => {
    expect(
      runsSessionFolderCommandOnSpace({
        text: "/add-to-folder",
        selectionStart: 14,
        selectionEnd: 14,
      }),
    ).toBe(true);
  });

  it("leaves edited selections and modified space shortcuts alone", () => {
    expect(
      runsSessionFolderCommandOnSpace({
        text: "/add-to-folder",
        selectionStart: 0,
        selectionEnd: 14,
      }),
    ).toBe(false);
    expect(
      runsSessionFolderCommandOnSpace({
        text: "/add-to-folder ",
        selectionStart: 15,
        selectionEnd: 15,
      }),
    ).toBe(false);
    expect(
      runsSessionFolderCommandOnSpace({
        text: "/add-to-folder",
        selectionStart: 14,
        selectionEnd: 14,
        metaKey: true,
      }),
    ).toBe(false);
  });
});
