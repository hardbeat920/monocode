import { createHash, randomUUID } from "node:crypto";
import {
  constants,
  existsSync,
  fstatSync,
  linkSync,
  lstatSync,
  mkdirSync,
  openSync,
  closeSync,
  readFileSync,
  readdirSync,
  renameSync,
  unlinkSync,
  writeFileSync,
} from "node:fs";
import { dirname, isAbsolute, join } from "node:path";
import type { Attachment } from "../src/features/sessions/model/session";
import type { ContextAssetSnapshot } from "../src/features/sessions/model/contextAssets";

const MAX_FILE_BYTES = 20 * 1024 * 1024;
const MAX_TOTAL_BYTES = 64 * 1024 * 1024;

export function snapshotHostContextAssets(
  directory: string,
  attachments: Attachment[],
  limits = { maxFileBytes: MAX_FILE_BYTES, maxTotalBytes: MAX_TOTAL_BYTES },
): ContextAssetSnapshot[] {
  if (!attachments.length) return [];
  mkdirSync(directory, { recursive: true, mode: 0o700 });
  const indexPath = join(dirname(directory), "assets.index.json");
  let savedSnapshots: ContextAssetSnapshot[] = [];
  if (existsSync(indexPath)) {
    try {
      const parsed: unknown = JSON.parse(readFileSync(indexPath, "utf8"));
      if (Array.isArray(parsed))
        savedSnapshots = parsed.filter(
          (entry): entry is ContextAssetSnapshot =>
            entry &&
            typeof entry.id === "string" &&
            typeof entry.sha256 === "string" &&
            /^[a-f0-9]{64}$/.test(entry.sha256) &&
            entry.path === join(directory, entry.sha256),
        );
    } catch {
      savedSnapshots = [];
    }
  }
  let totalBytes = readdirSync(directory).reduce((total, name) => {
    if (!/^[a-f0-9]{64}$/.test(name)) return total;
    const info = lstatSync(join(directory, name));
    return total + (info.isFile() ? info.size : 0);
  }, 0);
  const seen = new Set<string>();
  const snapshots: ContextAssetSnapshot[] = [];
  for (const attachment of attachments) {
    if (seen.has(attachment.id)) continue;
    seen.add(attachment.id);
    try {
      const retained = savedSnapshots.find(
        (entry) => entry.id === attachment.id,
      );
      if (retained?.path && retained.sha256) {
        const retainedInfo = lstatSync(retained.path);
        if (!retainedInfo.isFile() || retainedInfo.size > limits.maxFileBytes)
          throw new Error(
            "The saved attachment exceeds the snapshot file limit or is not a regular file",
          );
        const retainedBytes = readFileSync(retained.path);
        if (
          createHash("sha256").update(retainedBytes).digest("hex") !==
          retained.sha256
        )
          throw new Error("The saved attachment failed its content hash check");
        snapshots.push(retained);
        continue;
      }
      let data: Buffer;
      if (attachment.data) {
        if (attachment.data.length > Math.ceil(limits.maxFileBytes / 3) * 4 + 4)
          throw new Error(
            "The historical attachment exceeds the snapshot file limit",
          );
        data = Buffer.from(attachment.data, "base64");
      } else if (attachment.path) {
        if (!isAbsolute(attachment.path))
          throw new Error(
            "The historical attachment has no absolute host path",
          );
        if (!lstatSync(attachment.path).isFile())
          throw new Error("The historical attachment is not a regular file");
        const descriptor = openSync(
          attachment.path,
          constants.O_RDONLY |
            (constants.O_NOFOLLOW ?? 0) |
            (constants.O_NONBLOCK ?? 0),
        );
        try {
          const info = fstatSync(descriptor);
          if (!info.isFile())
            throw new Error("The historical attachment is not a regular file");
          if (info.size > limits.maxFileBytes)
            throw new Error(
              "The historical attachment exceeds the snapshot file limit",
            );
          if (info.size !== attachment.size)
            throw new Error(
              "The historical attachment changed size before it could be saved",
            );
          data = readFileSync(descriptor);
          if (data.length !== info.size)
            throw new Error(
              "The historical attachment changed while it was being saved",
            );
        } finally {
          closeSync(descriptor);
        }
      } else {
        throw new Error(
          "The historical attachment is unavailable on this host",
        );
      }
      if (data.length > limits.maxFileBytes)
        throw new Error(
          "The historical attachment exceeds the snapshot file limit",
        );
      const sha256 = createHash("sha256").update(data).digest("hex");
      const path = join(directory, sha256);
      if (existsSync(path)) {
        if (
          createHash("sha256").update(readFileSync(path)).digest("hex") !==
          sha256
        )
          throw new Error("The saved attachment failed its content hash check");
      } else {
        if (totalBytes + data.length > limits.maxTotalBytes)
          throw new Error(
            "The session attachment snapshots exceed the total size limit",
          );
        const temporary = join(directory, `.${randomUUID()}.tmp`);
        try {
          writeFileSync(temporary, data, {
            flag: "wx",
            mode: 0o400,
            flush: true,
          });
          try {
            linkSync(temporary, path);
          } catch (error) {
            if (!(
              error instanceof Error &&
              "code" in error &&
              error.code === "EEXIST"
            ))
              throw error;
            if (
              createHash("sha256").update(readFileSync(path)).digest("hex") !==
              sha256
            )
              throw new Error(
                "The saved attachment failed its content hash check",
              );
          }
        } finally {
          if (existsSync(temporary)) unlinkSync(temporary);
        }
        totalBytes += data.length;
      }
      snapshots.push({ id: attachment.id, path, sha256 });
    } catch (error) {
      const reason =
        error instanceof Error && "code" in error
          ? "The historical attachment cannot be read on this host"
          : error instanceof Error
            ? error.message
            : "The historical attachment cannot be saved on this host";
      snapshots.push({ id: attachment.id, unavailableReason: reason });
    }
  }
  const merged = new Map(savedSnapshots.map((entry) => [entry.id, entry]));
  for (const entry of snapshots)
    if (entry.path && entry.sha256) merged.set(entry.id, entry);
  const temporary = `${indexPath}.${randomUUID()}.tmp`;
  writeFileSync(temporary, JSON.stringify([...merged.values()]), {
    mode: 0o600,
    flag: "wx",
  });
  renameSync(temporary, indexPath);
  return snapshots;
}
