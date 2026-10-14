// @vitest-environment happy-dom
import { beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { readBinaryFile, REMOTE_PATH_PREFIX } from "./fs";
import { downloadFile } from "./download";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: vi.fn() }));
vi.mock("./fs", () => ({
  readBinaryFile: vi.fn(),
  REMOTE_PATH_PREFIX: "remote://",
}));

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(save).mockResolvedValue("/downloads/result.bin");
  vi.mocked(invoke).mockResolvedValue(undefined);
});

it("copies a local file directly without the preview size limit", async () => {
  await downloadFile("report.pdf", { path: "/workspace/report.pdf" });
  expect(save).toHaveBeenCalledWith({
    title: "Download file",
    defaultPath: "report.pdf",
  });
  expect(invoke).toHaveBeenCalledWith("save_download", {
    path: "/downloads/result.bin",
    from: "/workspace/report.pdf",
  });
  expect(readBinaryFile).not.toHaveBeenCalled();
});

it("cancels without reading or writing anything", async () => {
  vi.mocked(save).mockResolvedValue(null);
  await downloadFile("image.png", { path: "/workspace/image.png" });
  expect(invoke).not.toHaveBeenCalled();
  expect(readBinaryFile).not.toHaveBeenCalled();
});

it("saves remote bytes to the local destination", async () => {
  const path = `${REMOTE_PATH_PREFIX}machine/workspace/report.pdf`;
  vi.mocked(readBinaryFile).mockResolvedValue(new Uint8Array([0, 255, 42]));
  await downloadFile("report.pdf", { path });
  expect(readBinaryFile).toHaveBeenCalledWith(path);
  expect(invoke).toHaveBeenCalledWith("save_download", {
    path: "/downloads/result.bin",
    data: "AP8q",
  });
});

it("preserves binary data from inline attachments", async () => {
  await downloadFile("image.png", { url: "data:image/png;base64,AP8q" });
  expect(invoke).toHaveBeenCalledWith("save_download", {
    path: "/downloads/result.bin",
    data: "AP8q",
  });
});

it("propagates save failures for the control to display", async () => {
  vi.mocked(invoke).mockRejectedValue(new Error("Disk full"));
  await expect(
    downloadFile("report.pdf", { path: "/report.pdf" }),
  ).rejects.toThrow("Disk full");
});

it("writes artifact text directly without fetching or encoding a data URL", async () => {
  await downloadFile("report.md", { text: "Résumé 🎉\n# Findings" });
  expect(invoke).toHaveBeenCalledWith("write_text_file", {
    path: "/downloads/result.bin",
    content: "Résumé 🎉\n# Findings",
  });
});

it.each([
  ["/tmp/report.pdf", "report.pdf"],
  ["C:\\reports\\résumé.md", "résumé.md"],
  ["", "download"],
])("suggests a filename without a directory for %s", async (name, expected) => {
  await downloadFile(name, { text: "body" });
  expect(save).toHaveBeenCalledWith({
    title: "Download file",
    defaultPath: expected,
  });
});

it("preserves a larger binary payload", async () => {
  const bytes = Uint8Array.from({ length: 25000 }, (_, i) => i % 256);
  vi.mocked(readBinaryFile).mockResolvedValue(bytes);
  await downloadFile("large.bin", {
    path: `${REMOTE_PATH_PREFIX}machine/large.bin`,
  });
  const args = vi.mocked(invoke).mock.calls[0][1] as { data: string };
  expect(
    Uint8Array.from(atob(args.data), (char) => char.charCodeAt(0)),
  ).toEqual(bytes);
});
