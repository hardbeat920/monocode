// @vitest-environment happy-dom
import { afterEach, expect, it, vi } from "vitest";
import { copyMessage, messageFilesFromClipboard } from "./clipboard";
import { invoke } from "@tauri-apps/api/core";
import {
  MAX_EMBED_BYTES,
  attachmentsFromFiles,
  displayAttachments,
  persistableAttachment,
} from "../../features/sessions/model/attachments";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

afterEach(() => vi.restoreAllMocks());

it("copies fresh disk images but leaves their restored placeholders alone", async () => {
  vi.mocked(invoke).mockResolvedValue(btoa("<svg></svg>"));
  const write = vi.spyOn(navigator.clipboard, "write").mockResolvedValue();
  const files = displayAttachments([
    {
      id: "svg",
      name: "diagram.svg",
      mimeType: "image/svg+xml",
      kind: "image",
      size: 11,
      path: "/diagram.svg",
    },
  ]);
  await copyMessage("Diagram", files);
  expect(write).toHaveBeenCalledOnce();
  const html = await (
    await write.mock.calls[0][0][0].getType("text/html")
  ).text();
  const pasted = messageFilesFromClipboard({ getData: () => html });
  expect(await pasted![0].text()).toBe("<svg></svg>");
  write.mockClear();
  vi.mocked(invoke).mockClear();
  await copyMessage("Diagram", files.map(persistableAttachment));
  expect(write).not.toHaveBeenCalled();
  expect(invoke).not.toHaveBeenCalled();
  expect(await navigator.clipboard.readText()).toBe("Diagram");
});

it("copies a message with multiple attachments and preserves names and bytes on paste", async () => {
  let written: ClipboardItem[] = [];
  vi.spyOn(navigator.clipboard, "write").mockImplementation(async (items) => {
    written = items;
  });
  await copyMessage("Look at these", [
    {
      id: "1",
      name: "screen.png",
      mimeType: "image/png",
      kind: "image",
      size: 3,
      data: "YWJj",
    },
    {
      id: "2",
      name: "report.pdf",
      mimeType: "application/pdf",
      kind: "file",
      size: 3,
      data: "ZGVm",
    },
  ]);
  expect(await (await written[0].getType("text/plain")).text()).toBe(
    "Look at these",
  );
  const html = await (await written[0].getType("text/html")).text();
  const files = messageFilesFromClipboard({
    getData: (type) => (type === "text/html" ? html : ""),
  });
  expect(files?.map((file) => file.name)).toEqual(["screen.png", "report.pdf"]);
  expect(await files![0].text()).toBe("abc");
  expect(await files![1].text()).toBe("def");
  expect(
    Array.from(
      new Uint8Array(
        await (await written[0].getType("image/png")).arrayBuffer(),
      ),
    ),
  ).toEqual([97, 98, 99]);
});

it("rejects clipboard files whose decoded bytes exceed the attachment limit", () => {
  const base64Length = Math.ceil((MAX_EMBED_BYTES + 1) / 3) * 4;
  const payload = encodeURIComponent(
    JSON.stringify([
      {
        name: "oversized.bin",
        mimeType: "application/octet-stream",
        data: "A".repeat(base64Length),
      },
    ]),
  );
  const html = `<div data-monocode-files="${payload}"></div>`;

  expect(
    messageFilesFromClipboard({
      getData: (type) => (type === "text/html" ? html : ""),
    }),
  ).toBeNull();
});

it("skips restored images without live content while keeping the text", async () => {
  vi.mocked(invoke).mockImplementation(async (_command, args) => {
    if ((args as { path: string }).path === "/old.png") return "YWJj";
    throw new Error("File no longer exists");
  });
  await copyMessage("Old message", [
    {
      id: "1",
      name: "old.png",
      mimeType: "image/png",
      kind: "image",
      size: 3,
      path: "/old.png",
    },
  ]);
  expect(await navigator.clipboard.readText()).toBe("Old message");
  expect(invoke).not.toHaveBeenCalledWith("read_file_base64", {
    path: "/old.png",
  });
});

it("keeps the existing clipboard when a message has no copyable content", async () => {
  await navigator.clipboard.writeText("Previous clipboard");

  await expect(
    copyMessage("", [
      {
        id: "restored",
        name: "old.png",
        mimeType: "image/png",
        kind: "image",
        size: 3,
        path: "/old.png",
      },
    ]),
  ).rejects.toThrow("No copyable content");

  expect(await navigator.clipboard.readText()).toBe("Previous clipboard");
});

it("copies available disk attachments including empty files", async () => {
  vi.mocked(invoke).mockImplementation(async (_command, args) =>
    (args as { path: string }).path === "/empty.txt" ? "" : "cGRm",
  );
  let written: ClipboardItem[] = [];
  vi.spyOn(navigator.clipboard, "write").mockImplementation(async (items) => {
    written = items;
  });
  await copyMessage("Files", [
    {
      id: "1",
      name: "doc.pdf",
      mimeType: "application/pdf",
      kind: "file",
      size: 3,
      path: "/doc.pdf",
    },
    {
      id: "2",
      name: "empty.txt",
      mimeType: "text/plain",
      kind: "file",
      size: 0,
      path: "/empty.txt",
    },
  ]);
  const html = await (await written[0].getType("text/html")).text();
  const files = messageFilesFromClipboard({ getData: () => html });
  expect(files?.map((file) => file.name)).toEqual(["doc.pdf", "empty.txt"]);
  expect(await files![0].text()).toBe("pdf");
  expect(files![1].size).toBe(0);
  vi.mocked(invoke).mockResolvedValue("/pasted.txt");
  const pasted = await attachmentsFromFiles(files!);
  expect(pasted.map((file) => file.name)).toEqual(["doc.pdf", "empty.txt"]);
});

it.each([
  "File is too large to attach inline (maximum 20 MB).",
  "File no longer exists",
])(
  "does not replace the clipboard with a partial message when a file cannot be read: %s",
  async (reason) => {
    vi.mocked(invoke).mockRejectedValue(new Error(reason));
    await navigator.clipboard.writeText("Previous clipboard");
    await expect(
      copyMessage("New message", [
        {
          id: "pdf",
          name: "report.pdf",
          mimeType: "application/pdf",
          kind: "file",
          size: 21 * 1024 * 1024,
          path: "/report.pdf",
        },
      ]),
    ).rejects.toThrow("report.pdf");
    expect(await navigator.clipboard.readText()).toBe("Previous clipboard");
  },
);
