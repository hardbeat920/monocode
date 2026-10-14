// @vitest-environment happy-dom
import { beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import { invokeWorkspace, REMOTE_PATH_PREFIX } from "./fs";
import { downloadFile } from "./download";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: vi.fn() }));
vi.mock("./fs", () => ({
  invokeWorkspace: vi.fn(),
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
  expect(invokeWorkspace).not.toHaveBeenCalled();
});

it("cancels without reading or writing anything", async () => {
  vi.mocked(save).mockResolvedValue(null);
  await downloadFile("image.png", { path: "/workspace/image.png" });
  expect(invoke).not.toHaveBeenCalled();
  expect(invokeWorkspace).not.toHaveBeenCalled();
});

it("routes remote files to native streaming without reading bytes into the renderer", async () => {
  const path = `${REMOTE_PATH_PREFIX}machine/workspace/report.pdf`;
  await downloadFile("report.pdf", { path });
  expect(invokeWorkspace).toHaveBeenCalledWith("download_file", {
    path,
    destination: "/downloads/result.bin",
  });
  expect(invoke).not.toHaveBeenCalled();
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
  expect(invoke).toHaveBeenCalledWith("save_text_download", {
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
