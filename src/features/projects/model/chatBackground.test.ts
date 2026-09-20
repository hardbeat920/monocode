import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  clearProjectChatBackground,
  pickAndSaveChatBackground,
  pickAndSaveProjectChatBackground,
  projectChatBackgroundSrc,
  removeChatBackground,
} from "./chatBackground";

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  open: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: (path: string) => `asset://${path}`,
  invoke: mocks.invoke,
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: mocks.open }));

describe("chat background image", () => {
  beforeEach(() => {
    mocks.invoke.mockReset();
    mocks.open.mockReset();
  });

  it("copies a picked image into app storage", async () => {
    mocks.open.mockResolvedValue("/Pictures/aurora.webp");
    mocks.invoke.mockResolvedValue(
      "/app-data/backgrounds/chat-background.webp",
    );

    await expect(pickAndSaveChatBackground()).resolves.toBe(
      "/app-data/backgrounds/chat-background.webp",
    );
    expect(mocks.invoke).toHaveBeenCalledWith("save_chat_background", {
      sourcePath: "/Pictures/aurora.webp",
    });
  });

  it("leaves the current background alone when picking is cancelled", async () => {
    mocks.open.mockResolvedValue(null);
    await expect(pickAndSaveChatBackground()).resolves.toBeNull();
    expect(mocks.invoke).not.toHaveBeenCalled();
  });

  it("removes the saved background", async () => {
    mocks.invoke.mockResolvedValue(undefined);
    await removeChatBackground();
    expect(mocks.invoke).toHaveBeenCalledWith("remove_chat_background");
  });

  it("copies a picked image into project-specific app storage", async () => {
    mocks.open.mockResolvedValue("/Pictures/grid.png");
    mocks.invoke.mockResolvedValue("/app-data/backgrounds/project-abc.png");

    await expect(
      pickAndSaveProjectChatBackground("/work/agent-terminal"),
    ).resolves.toBe("/app-data/backgrounds/project-abc.png");
    expect(mocks.invoke).toHaveBeenCalledWith("save_project_chat_background", {
      project: "/work/agent-terminal",
      sourcePath: "/Pictures/grid.png",
    });
  });

  it("removes only the selected project's saved background", async () => {
    mocks.invoke.mockResolvedValue(undefined);

    await clearProjectChatBackground("/work/agent-terminal");

    expect(mocks.invoke).toHaveBeenCalledWith(
      "remove_project_chat_background",
      { project: "/work/agent-terminal" },
    );
  });

  it("cache-busts project background URLs after replacement", () => {
    expect(projectChatBackgroundSrc("/app-data/background.png", 42)).toBe(
      "asset:///app-data/background.png?v=42",
    );
  });
});
