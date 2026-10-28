import { expect, it } from "vitest";
import { SyncTransfers } from "./sync-transfer";
import type { SessionSync } from "../src/features/connections/model/protocol";

const sync = (text: string): SessionSync => ({
  kind: "delta",
  base: 1,
  value: {
    projectId: "project",
    revision: 2,
    status: "idle",
    updatedAt: 0,
    session: {
      id: "session",
      harness: "codex",
      model: "codex:test",
      runtimeMode: "supervised",
      cwd: "/repo",
      title: "Session",
    },
  },
  blockIds: ["block"],
  blocks: [{ id: "block", role: "assistant", text }],
});

function read(transfers: SyncTransfers, transfer: string, length: number) {
  const pieces: string[] = [];
  for (let offset = 0; offset < length;) {
    const { data } = transfers.chunk("session", transfer, offset);
    pieces.push(data);
    offset += data.length;
  }
  return pieces;
}

it("returns small syncs inline", () => {
  const transfers = new SyncTransfers({ inline: 1024, chunk: 256 });
  expect(transfers.respond("session", sync("short"))).toEqual(sync("short"));
});

it("splits large syncs into bounded pieces without splitting characters", () => {
  const transfers = new SyncTransfers({ inline: 1024, chunk: 256 });
  const value = sync('🚀"\u0001'.repeat(2_000));
  const response = transfers.respond("session", value);
  if (response.kind !== "chunked") throw new Error("Expected a transfer");
  const pieces = read(transfers, response.transfer, response.length);
  expect(pieces.length).toBeGreaterThan(10);
  for (const piece of pieces) {
    expect(Buffer.byteLength(JSON.stringify(piece))).toBeLessThanOrEqual(256);
    expect(piece).not.toMatch(
      /[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/,
    );
  }
  expect(JSON.parse(pieces.join(""))).toEqual(value);
});

it("serves a transfer only for its session and forgets it after the last piece", () => {
  const transfers = new SyncTransfers({ inline: 64, chunk: 4096 });
  const response = transfers.respond("session", sync("x".repeat(500)));
  if (response.kind !== "chunked") throw new Error("Expected a transfer");
  expect(() => transfers.chunk("other", response.transfer, 0)).toThrow(
    "expired",
  );
  expect(() => transfers.chunk("session", response.transfer, -1)).toThrow(
    "offset",
  );
  read(transfers, response.transfer, response.length);
  expect(() => transfers.chunk("session", response.transfer, 0)).toThrow(
    "expired",
  );
});
