import { beforeEach, describe, expect, it, vi } from "vitest";
const invoke = vi.hoisted(() => vi.fn(async () => []));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import { historicalContextAttachments, snapshotContextAssets, snapshotPortableContextAssets } from "./contextAssets";
import { buildPortableContext, buildPortableContextSnapshot } from "./portableContext";
import { newSession, type Attachment } from "./session";

describe("durable historical attachment references", () => {
  beforeEach(() => invoke.mockClear());

  it("passes only original file or byte sources to the owning desktop command", async () => {
    const attachments: Attachment[] = [{ id: "a", name: "image.png", kind: "image", mimeType: "image/png", size: 4, data: "bytes", path: "/original/image.png", previewUrl: "blob:temporary", copyFromPath: true }];
    await snapshotContextAssets("session-1", attachments);
    expect(invoke).toHaveBeenCalledWith("session_context_assets", { sessionId: "session-1", attachments: [{ id: "a", name: "image.png", path: "/original/image.png", data: "bytes" }] });
    expect(attachments[0].previewUrl).toBe("blob:temporary");
  });

  it("collects only attachments in settled eligible history before the frozen boundary", () => {
    const attachment = (id: string): Attachment => ({ id, name: `${id}.png`, kind: "image", mimeType: "image/png", size: 4, data: "bytes" });
    const session = { ...newSession("claude", "/repo"), blocks: [
      { id: "u", role: "user" as const, text: "submitted", attachments: [attachment("user-image")] },
      { id: "draft", role: "user" as const, text: "draft", draft: true, attachments: [attachment("draft-image")] },
      { id: "internal", role: "user" as const, text: "internal", internal: true, attachments: [attachment("internal-image")] },
      { id: "generated", role: "image" as const, text: "", image: { name: "generated.png", path: "/original/generated.png", mimeType: "image/png", size: 4 } },
      { id: "future", role: "user" as const, text: "later", attachments: [attachment("future-image")] },
    ] };
    expect(historicalContextAttachments(session, "generated").map((item) => item.id)).toEqual(["user-image", "generated"]);
  });

  it("rewrites portable references and omissions without changing original transcript paths", () => {
    const attachment: Attachment = { id: "a", name: "file.txt", kind: "file", mimeType: "text/plain", size: 4, path: "/temporary/file.txt" };
    const session = { ...newSession("claude", "/repo"), blocks: [{ id: "u", role: "user" as const, text: "Inspect the file", attachments: [attachment] }] };
    const context = buildPortableContext(session);
    const snapshots = [{ id: "a", path: "/app-data/context-history/session/assets/hash.txt", sha256: "hash" }];
    const saved = snapshotPortableContextAssets(context, snapshots);
    expect(saved.items[0].attachments?.[0]).toMatchObject(snapshots[0]);
    expect(context.items[0].attachments?.[0].path).toBe("/temporary/file.txt");
    expect(attachment.path).toBe("/temporary/file.txt");
    expect(buildPortableContextSnapshot(session, "u", snapshots)).toContain("/app-data/context-history/session/assets/hash.txt");
    const unavailable = snapshotPortableContextAssets(context, [{ id: "a", unavailableReason: "The source file is missing" }]);
    expect(unavailable.items[0].attachments?.[0].path).toBeUndefined();
    expect(unavailable.items[0].attachments?.[0].unavailableReason).toBe("The source file is missing");
  });

  it("includes saved paths in byte accounting before selecting history", () => {
    const session = { ...newSession("claude", "/repo"), blocks: [{ id: "u", role: "user" as const, text: "Inspect the file", attachments: [{ id: "a", name: "file.txt", kind: "file" as const, mimeType: "text/plain", size: 4, path: "/short" }] }] };
    expect(buildPortableContext(session, { maxBytes: 1_800 }).items).toHaveLength(1);
    const snapshots = [{ id: "a", path: `/${"long-path-segment/".repeat(200)}asset.txt`, sha256: "hash" }];
    const budgeted = buildPortableContext(session, { maxBytes: 1_800, assetSnapshots: snapshots });
    expect(budgeted.items).toEqual([]);
    expect(budgeted.omitted).toContainEqual({ id: `${session.id}:u`, reason: "budget" });
  });
});
