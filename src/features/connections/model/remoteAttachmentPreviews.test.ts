import { expect, it, vi } from "vitest";
import { withRemoteAttachmentPreviews } from "./remoteAttachmentPreviews";
import type { HostSession } from "./protocol";

const snapshot = (): HostSession => ({
  projectId: "project",
  revision: 1,
  updatedAt: 0,
  status: "idle",
  session: {
    id: "session",
    cwd: "/repo",
    harness: "codex",
    model: "codex:test",
    runtimeMode: "supervised",
    title: "Image",
    blocks: [
      {
        id: "turn",
        role: "user",
        text: "Look",
        attachments: [
          {
            id: "image",
            name: "shot.png",
            mimeType: "image/png",
            kind: "image",
            size: 5,
            path: "/host/image",
          },
        ],
      },
    ],
  },
});

it("reopens image previews from chunks and reuses bytes on subsequent syncs", async () => {
  const read = vi.fn(async ({ offset }: { offset: number }) =>
    offset === 0
      ? { offset: 3, size: 5, data: btoa("abc") }
      : { offset: 5, size: 5, data: btoa("de") },
  );
  const first = await withRemoteAttachmentPreviews(
    "machine",
    snapshot(),
    undefined,
    read,
  );
  expect(first.session.blocks[0].attachments?.[0].data).toBe(btoa("abcde"));
  expect(read).toHaveBeenCalledTimes(2);
  const next = await withRemoteAttachmentPreviews(
    "machine",
    snapshot(),
    first,
    read,
  );
  expect(next.session.blocks[0].attachments?.[0].data).toBe(btoa("abcde"));
  expect(read).toHaveBeenCalledTimes(2);
});

it("keeps the transcript available when an image is missing", async () => {
  const value = snapshot();
  const read = vi.fn(async () => {
    throw new Error("Image no longer available");
  });
  expect(
    await withRemoteAttachmentPreviews("machine", value, undefined, read),
  ).toBe(value);
});
