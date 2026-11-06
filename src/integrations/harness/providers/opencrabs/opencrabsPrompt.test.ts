import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Attachment } from "../../../../features/sessions/model/session";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (command: string, args: unknown) => invokeMock(command, args),
}));

import { openCrabsPromptBlocks } from "./opencrabsPrompt";

function imageAttachment(over: Partial<Attachment>): Attachment {
  return {
    id: "att-1",
    name: "shot.png",
    mimeType: "image/png",
    kind: "image",
    size: 4,
    ...over,
  } as Attachment;
}

describe("openCrabsPromptBlocks", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("passes plain text through unchanged", async () => {
    const blocks = await openCrabsPromptBlocks("hello", []);
    expect(blocks).toEqual([{ type: "text", text: "hello" }]);
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("rewrites a disk-backed image as a resource_link without persisting", async () => {
    const file = imageAttachment({ path: "/tmp/shot.png", data: "aW1hZ2U=" });
    const blocks = await openCrabsPromptBlocks("look", [file]);
    expect(blocks[1]).toEqual({
      type: "resource_link",
      uri: "file:///tmp/shot.png",
      name: "shot.png",
      mimeType: "image/png",
      size: 4,
    });
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("persists a pasted blob and links the temp path", async () => {
    invokeMock.mockResolvedValue("/tmp/monocode-attachments/1-2-pasted-image.png");
    const file = imageAttachment({ data: "aW1hZ2U=" });
    const blocks = await openCrabsPromptBlocks("", [file]);
    expect(invokeMock).toHaveBeenCalledWith("write_attachment", {
      name: "pasted-image.png",
      data: "aW1hZ2U=",
    });
    expect(blocks[0]).toEqual({
      type: "resource_link",
      uri: "file:///tmp/monocode-attachments/1-2-pasted-image.png",
      name: "pasted-image.png",
      mimeType: "image/png",
      size: 4,
    });
  });

  it("fails loud when a pasted blob cannot be persisted", async () => {
    invokeMock.mockRejectedValue(new Error("disk full"));
    const file = imageAttachment({ data: "aW1hZ2U=" });
    await expect(openCrabsPromptBlocks("", [file])).rejects.toThrow(
      "disk full",
    );
  });

  it("leaves non-vision resource_link attachments untouched", async () => {
    const file: Attachment = {
      id: "att-2",
      name: "notes.md",
      mimeType: "text/markdown",
      kind: "attachment",
      size: 10,
      path: "/tmp/notes.md",
    } as Attachment;
    const blocks = await openCrabsPromptBlocks("", [file]);
    expect(blocks[0]).toEqual({
      type: "resource_link",
      uri: "file:///tmp/notes.md",
      name: "notes.md",
      mimeType: "text/markdown",
      size: 10,
    });
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("links a disk-backed audio attachment without persisting", async () => {
    const file: Attachment = {
      id: "att-3",
      name: "voice.mp3",
      mimeType: "audio/mpeg",
      kind: "audio",
      size: 42,
      path: "/tmp/voice.mp3",
    } as Attachment;
    const blocks = await openCrabsPromptBlocks("transcribe this", [file]);
    expect(blocks[1]).toEqual({
      type: "resource_link",
      uri: "file:///tmp/voice.mp3",
      name: "voice.mp3",
      mimeType: "audio/mpeg",
      size: 42,
    });
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("persists a pasted audio blob with a kind-specific name", async () => {
    invokeMock.mockResolvedValue("/tmp/monocode-attachments/3-pasted-audio.mp3");
    const file: Attachment = {
      id: "att-4",
      name: "voice.mp3",
      mimeType: "audio/mpeg",
      kind: "audio",
      size: 42,
      data: "YXVkaW8=",
    } as Attachment;
    const blocks = await openCrabsPromptBlocks("", [file]);
    expect(invokeMock).toHaveBeenCalledWith("write_attachment", {
      name: "pasted-audio.mp3",
      data: "YXVkaW8=",
    });
    expect(blocks[0]).toEqual({
      type: "resource_link",
      uri: "file:///tmp/monocode-attachments/3-pasted-audio.mp3",
      name: "pasted-audio.mp3",
      mimeType: "audio/mpeg",
      size: 42,
    });
  });

  it("fails loud when an attachment has neither path nor data", async () => {
    const file: Attachment = {
      id: "att-5",
      name: "ghost.pdf",
      mimeType: "application/pdf",
      kind: "file",
      size: 1,
    } as Attachment;
    await expect(openCrabsPromptBlocks("", [file])).rejects.toThrow(
      "no local file path or data",
    );
    expect(invokeMock).not.toHaveBeenCalled();
  });
});

describe("spawnArgs (ACP child spawn)", () => {
  // The wire-proven dogfood leak: the "Default" catalog placeholder spawned
  // `--model default`, which only worked by server fallback luck.
  it("omits --model when the placeholder id is selected", async () => {
    const { spawnArgs } = await import("./opencrabs");
    expect(spawnArgs("default")).toEqual(["acp"]);
    expect(spawnArgs("Default")).toEqual(["acp"]);
  });

  it("passes a real model pair through", async () => {
    const { spawnArgs } = await import("./opencrabs");
    // Pairs pass through whole — the server's set_model is pair-aware.
    expect(spawnArgs("infer/ali/glm-5.3")).toEqual([
      "acp",
      "--model",
      "infer/ali/glm-5.3",
    ]);
  });
});
