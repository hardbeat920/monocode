import { afterEach, expect, it, vi } from "vitest";
import { createHash } from "node:crypto";
import {
  mkdtempSync,
  existsSync,
  linkSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
  symlinkSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { snapshotHostContextAssets } from "./context-assets";
import type { Attachment } from "../src/features/sessions/model/session";

vi.mock("node:fs", async (importOriginal) => {
  const actual = await importOriginal<typeof import("node:fs")>();
  return {
    ...actual,
    writeFileSync: vi.fn(actual.writeFileSync),
    linkSync: vi.fn(actual.linkSync),
  };
});

const directories: string[] = [];
afterEach(() => {
  vi.mocked(writeFileSync).mockReset();
  vi.mocked(linkSync).mockReset();
  directories
    .splice(0)
    .forEach((directory) =>
      rmSync(directory, { recursive: true, force: true }),
    );
});

function setup() {
  const directory = mkdtempSync(join(tmpdir(), "monocode-context-assets-"));
  directories.push(directory);
  const original = join(directory, "original.txt");
  writeFileSync(original, "original");
  const attachment: Attachment = {
    id: "file",
    path: original,
    size: 8,
    name: "original.txt",
    mimeType: "text/plain",
    kind: "file",
  };
  return { directory, original, attachment, assets: join(directory, "assets") };
}

it("saves content by hash and preserves it after the original file changes", () => {
  const s = setup();
  const [saved] = snapshotHostContextAssets(s.assets, [
    s.attachment,
    s.attachment,
  ]);
  expect(saved.sha256).toBe(
    createHash("sha256").update("original").digest("hex"),
  );
  expect(saved.path).toBe(join(s.assets, saved.sha256!));
  writeFileSync(s.original, "modified");
  expect(readFileSync(saved.path!, "utf8")).toBe("original");
  unlinkSync(s.original);
  expect(snapshotHostContextAssets(s.assets, [s.attachment])[0]).toEqual(saved);
  const mode = statSync(saved.path!).mode;
  expect(mode & 0o222).toBe(0);
  if (process.platform !== "win32") expect(mode & 0o777).toBe(0o400);
});

it("deduplicates matching content without charging the total limit twice", () => {
  const s = setup();
  const snapshots = snapshotHostContextAssets(
    s.assets,
    [s.attachment, { ...s.attachment, id: "same-file" }],
    { maxFileBytes: 8, maxTotalBytes: 8 },
  );
  expect(snapshots).toHaveLength(2);
  expect(snapshots[0].path).toBe(snapshots[1].path);
  expect(snapshots.every((entry) => !entry.unavailableReason)).toBe(true);
});

it("keeps a partial write outside the published hash path and allows retry", async () => {
  const s = setup();
  const actual = await vi.importActual<typeof import("node:fs")>("node:fs");
  const finalPath = join(
    s.assets,
    createHash("sha256").update("original").digest("hex"),
  );
  let exposedPartialBytes = false;
  vi.mocked(writeFileSync).mockImplementationOnce((path, data, options) => {
    actual.writeFileSync(path, (data as Buffer).subarray(0, 3), options);
    exposedPartialBytes = existsSync(finalPath);
    throw new Error("Interrupted asset write");
  });
  const [failed] = snapshotHostContextAssets(s.assets, [s.attachment]);
  expect(exposedPartialBytes).toBe(false);
  expect(failed.path).toBeUndefined();
  expect(failed.unavailableReason).toContain("Interrupted asset write");
  expect(existsSync(finalPath)).toBe(false);
  expect(readdirSync(s.assets)).toEqual([]);
  const [saved] = snapshotHostContextAssets(s.assets, [s.attachment]);
  expect(saved.path).toBe(finalPath);
  expect(readFileSync(finalPath, "utf8")).toBe("original");
});

it.each([true, false])(
  "preserves a racing publisher's existing file with matching content %s",
  async (matching) => {
    const s = setup();
    const actual = await vi.importActual<typeof import("node:fs")>("node:fs");
    const contents = matching ? "original" : "different";
    const finalPath = join(
      s.assets,
      createHash("sha256").update("original").digest("hex"),
    );
    vi.mocked(linkSync).mockImplementationOnce((_temporary, destination) => {
      actual.writeFileSync(destination, contents, { flag: "wx", mode: 0o400 });
      throw Object.assign(new Error("Existing destination"), {
        code: "EEXIST",
      });
    });
    const [saved] = snapshotHostContextAssets(s.assets, [s.attachment]);
    expect(linkSync).toHaveBeenCalledOnce();
    expect(readFileSync(finalPath, "utf8")).toBe(contents);
    expect(readdirSync(s.assets)).toEqual([
      finalPath.slice(s.assets.length + 1),
    ]);
    if (matching) expect(saved.path).toBe(finalPath);
    else {
      expect(saved.path).toBeUndefined();
      expect(saved.unavailableReason).toContain("content hash check");
    }
  },
);

it("reports missing files, changed files, symbolic links, and both size limits", () => {
  const s = setup();
  const link = join(s.directory, "link");
  symlinkSync(s.original, link);
  const snapshots = snapshotHostContextAssets(s.assets, [
    { ...s.attachment, id: "missing", path: join(s.directory, "missing") },
    { ...s.attachment, id: "changed", size: 7 },
    { ...s.attachment, id: "link", path: link },
  ]);
  expect(
    snapshots.every(
      (entry) => !!entry.unavailableReason && !entry.path && !entry.sha256,
    ),
  ).toBe(true);
  expect(
    snapshotHostContextAssets(s.assets, [s.attachment], {
      maxFileBytes: 7,
      maxTotalBytes: 8,
    })[0].unavailableReason,
  ).toContain("file limit");
  expect(
    snapshotHostContextAssets(s.assets, [s.attachment], {
      maxFileBytes: 8,
      maxTotalBytes: 7,
    })[0].unavailableReason,
  ).toContain("total size limit");
});

it("saves retained image bytes when the historical attachment has no source path", () => {
  const s = setup();
  const [saved] = snapshotHostContextAssets(s.assets, [
    {
      ...s.attachment,
      path: undefined,
      data: Buffer.from("original").toString("base64"),
    },
  ]);
  expect(readFileSync(saved.path!, "utf8")).toBe("original");
});

it("uses retained submitted bytes when the original file has changed", () => {
  const s = setup();
  writeFileSync(s.original, "modified");
  const [saved] = snapshotHostContextAssets(s.assets, [
    { ...s.attachment, data: Buffer.from("original").toString("base64") },
  ]);
  expect(readFileSync(saved.path!, "utf8")).toBe("original");
});
