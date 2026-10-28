import { expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { uploadRemoteAttachments } from "./remoteAttachments";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

it("uploads local bytes before returning host attachment references", async () => {
  const calls: Array<{ method: string; params: Record<string, unknown> }> = [];
  vi.mocked(invoke).mockImplementation(async (command, input) => {
    if (command === "read_file_base64")
      return Buffer.from("sample").toString("base64");
    if (command === "remote_request") {
      const { method, params } = input as {
        method: string;
        params: Record<string, unknown>;
      };
      calls.push({ method, params });
      return { offset: 6 };
    }
    throw new Error(`Unexpected invoke ${command}`);
  });
  const file = {
    id: "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
    name: "sample.txt",
    mimeType: "text/plain",
    kind: "file" as const,
    size: 6,
    path: "/laptop/sample.txt",
  };
  const refs = await uploadRemoteAttachments("machine", [file]);
  expect(refs).toEqual([
    {
      id: file.id,
      name: file.name,
      mimeType: file.mimeType,
      kind: file.kind,
      size: 6,
    },
  ]);
  expect(calls).toEqual([
    {
      method: "attachments.upload",
      params: {
        id: file.id,
        offset: 0,
        size: 6,
        data: Buffer.from("sample").toString("base64"),
      },
    },
  ]);
});
