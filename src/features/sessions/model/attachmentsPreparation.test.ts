// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import {
  attachmentsFromFiles,
  attachmentsFromPaths,
  MAX_EMBED_BYTES,
  persistableAttachment,
  prepareAttachments,
} from "./attachments";

beforeEach(() => {
  invoke.mockReset();
});

describe("file attachment preparation", () => {
  it.each([
    ["report.pdf", "application/pdf"],
    ["transcript.md", "text/markdown"],
    ["server.log", "text/plain"],
  ])(
    "persists a pasted %s before handing it to the harness",
    async (name, mimeType) => {
      const path = `/tmp/monocode-attachments/${name}`;
      invoke.mockResolvedValue(path);
      const files = await attachmentsFromFiles([
        new File(["contents"], name, { type: mimeType }),
      ]);
      expect(invoke).toHaveBeenCalledWith("write_attachment", {
        name,
        data: "Y29udGVudHM=",
      });
      expect(files).toHaveLength(1);
      expect(files[0]).toMatchObject({ name, mimeType, kind: "file", path });
      expect(files[0].data).toBeUndefined();
      expect(await prepareAttachments(files)).toEqual(files);
      expect(persistableAttachment(files[0]).path).toBe(path);
      expect(invoke).toHaveBeenCalledTimes(1);
    },
  );

  it("keeps a large volatile file as a path when snapshot mode cannot read it", async () => {
    const file = new File([], "Screenshot.png", { type: "image/png" });
    Object.defineProperty(file, "path", { value: "/tmp/Screenshot.png" });
    Object.defineProperty(file, "size", { value: MAX_EMBED_BYTES + 1 });
    invoke.mockResolvedValue([
      {
        path: "/tmp/Screenshot.png",
        name: "Screenshot.png",
        size: MAX_EMBED_BYTES + 1,
        isDir: false,
      },
    ]);
    const files = await attachmentsFromFiles([file], { snapshot: true });
    expect(files[0]).toMatchObject({ path: "/tmp/Screenshot.png" });
    expect(invoke).toHaveBeenCalledWith("inspect_paths", {
      paths: ["/tmp/Screenshot.png"],
    });
  });

  it("keeps large disk documents as paths without reading them into the prompt", async () => {
    invoke.mockResolvedValue([
      {
        path: "/tmp/large.md",
        name: "large.md",
        size: MAX_EMBED_BYTES + 1,
        isDir: false,
      },
    ]);
    const files = await prepareAttachments(
      await attachmentsFromPaths(["/tmp/large.md"]),
    );
    expect(files[0]).toMatchObject({
      path: "/tmp/large.md",
      mimeType: "text/markdown",
    });
    expect(files[0].data).toBeUndefined();
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it("retains the image path when byte loading fails", async () => {
    invoke.mockImplementation(async (command: string) => {
      if (command === "inspect_paths")
        return [
          {
            path: "/tmp/image.png",
            name: "image.png",
            size: 100,
            isDir: false,
          },
        ];
      throw new Error("read failed");
    });
    const files = await prepareAttachments(
      await attachmentsFromPaths(["/tmp/image.png"]),
    );
    expect(files[0]).toMatchObject({
      path: "/tmp/image.png",
      mimeType: "image/png",
    });
    expect(files[0].data).toBeUndefined();
  });

  it("skips the base64 backfill for print-mode CLIs that only read paths", async () => {
    const files = [
      {
        id: "img",
        name: "red.png",
        mimeType: "image/png",
        kind: "image",
        size: 95,
        path: "/tmp/monocode-attachments/red.png",
      },
    ] as const;
    invoke.mockResolvedValue("aGVsbG8=");
    const skipped = await prepareAttachments([...files], { skipEmbed: true });
    expect(skipped[0].data).toBeUndefined();
    expect(skipped[0].path).toBe("/tmp/monocode-attachments/red.png");
    expect(invoke).not.toHaveBeenCalledWith("read_file_base64", {
      path: "/tmp/monocode-attachments/red.png",
    });

    invoke.mockClear();
    invoke.mockResolvedValue("aGVsbG8=");
    const embedded = await prepareAttachments([...files]);
    expect(embedded[0].data).toBe("aGVsbG8=");
    expect(invoke).toHaveBeenCalledWith("read_file_base64", {
      path: "/tmp/monocode-attachments/red.png",
    });
  });
});
