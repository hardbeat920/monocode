import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Attachment } from "../session";

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
});
