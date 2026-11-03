// @vitest-environment happy-dom
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { hasMobileAttachments, mobileAttachments } from "./mobileAttachments";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

beforeEach(() => {
  vi.mocked(invoke).mockReset().mockResolvedValue("/tmp/monocode-attachments/notes.txt");
  URL.createObjectURL = vi.fn(() => "blob:preview");
  URL.revokeObjectURL = vi.fn();
});

describe("mobileAttachments", () => {
  it("keeps a photo inline and writes other files to disk", async () => {
    const [photo, notes] = await mobileAttachments([
      { name: "shot.png", mimeType: "image/png", data: btoa("png") },
      { name: "notes.txt", mimeType: "text/plain", data: btoa("hello") },
    ]);
    expect(photo).toMatchObject({ name: "shot.png", kind: "image", data: btoa("png"), size: 3 });
    expect(photo.path).toBeUndefined();
    expect(notes).toMatchObject({
      name: "notes.txt",
      kind: "file",
      path: "/tmp/monocode-attachments/notes.txt",
    });
    expect(invoke).toHaveBeenCalledWith("write_attachment", { name: "notes.txt", data: btoa("hello") });
  });

  it("has none when the phone sent none", async () => {
    expect(hasMobileAttachments(undefined)).toBe(false);
    expect(hasMobileAttachments([])).toBe(false);
    await expect(mobileAttachments(undefined)).resolves.toEqual([]);
  });

  it("refuses malformed or too many files", async () => {
    await expect(mobileAttachments([{ name: "", data: "" }])).rejects.toThrow("name");
    await expect(mobileAttachments([{ name: "a.png", data: "%%%" }])).rejects.toThrow("could not be read");
    const many = Array.from({ length: 11 }, (_, i) => ({ name: `${i}.txt`, data: "" }));
    await expect(mobileAttachments(many)).rejects.toThrow("up to 10");
  });
});
