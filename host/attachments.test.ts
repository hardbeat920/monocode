import { afterEach, expect, it } from "vitest";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { HostStore } from "./store";
import { writeAttachmentChunk } from "./attachments";

const cleanups: Array<() => void> = [];
afterEach(() => cleanups.splice(0).forEach((cleanup) => cleanup()));

it("accepts ordered chunks and an identical retry while rejecting changes", () => {
  const directory = mkdtempSync(join(tmpdir(), "remote-upload-test-"));
  const store = new HostStore(join(directory, "host.db"));
  cleanups.push(() => {
    store.close();
    rmSync(directory, { recursive: true, force: true });
  });
  const id = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
  const chunk = (offset: number, data: string) =>
    writeAttachmentChunk(store, {
      id,
      offset,
      size: 6,
      data: Buffer.from(data).toString("base64"),
    });
  expect(chunk(0, "abc")).toEqual({ offset: 3 });
  expect(chunk(0, "abc")).toEqual({ offset: 3 });
  expect(() => chunk(0, "xyz")).toThrow("does not match");
  expect(() => chunk(4, "ef")).toThrow("out of order");
  expect(chunk(3, "def")).toEqual({ offset: 6 });
  expect(() =>
    writeAttachmentChunk(store, {
      id: "../escape",
      offset: 0,
      size: 1,
      data: "YQ==",
    }),
  ).toThrow("Invalid attachment ID");
});
