import type { HostSession } from "./protocol";

type Chunk = { data: string; offset: number; size: number };
const downloads = new Map<string, Promise<string>>();

/** Preview bytes live only in desktop snapshots, not in every host database
 * write. Unchanged attachments reuse their previous data across delta syncs. */
export async function withRemoteAttachmentPreviews(
  machineId: string,
  snapshot: HostSession,
  known: HostSession | undefined,
  read: (params: {
    sessionId: string;
    id: string;
    offset: number;
  }) => Promise<Chunk>,
): Promise<HostSession> {
  const previous = new Map(
    (known?.session.id === snapshot.session.id ? known.session.blocks : [])
      .flatMap((block) => block.attachments ?? [])
      .map((file) => [file.id, file]),
  );
  let changed = false;
  const blocks = await Promise.all(
    snapshot.session.blocks.map(async (block) => {
      if (!block.attachments?.length) return block;
      const attachments = await Promise.all(
        block.attachments.map(async (file) => {
          if (
            file.kind !== "image" ||
            file.data ||
            file.previewUrl ||
            file.size > 20 * 1024 * 1024
          )
            return file;
          try {
            let data = previous.get(file.id)?.data;
            if (data === undefined) {
              const key = JSON.stringify([
                machineId,
                snapshot.session.id,
                file.id,
              ]);
              let download = downloads.get(key);
              if (!download) {
                download = (async () => {
                  const pieces: string[] = [];
                  let offset = 0;
                  while (offset < file.size) {
                    const chunk = await read({
                      sessionId: snapshot.session.id,
                      id: file.id,
                      offset,
                    });
                    if (
                      chunk.size !== file.size ||
                      chunk.offset <= offset ||
                      chunk.offset > file.size ||
                      atob(chunk.data).length !== chunk.offset - offset ||
                      (chunk.offset < file.size &&
                        (chunk.offset - offset) % 3 !== 0)
                    )
                      throw new Error("Invalid image transfer");
                    pieces.push(chunk.data);
                    offset = chunk.offset;
                  }
                  return pieces.join("");
                })().finally(() => downloads.delete(key));
                downloads.set(key, download);
              }
              data = await download;
            }
            return { ...file, data };
          } catch {
            // Missing images must not hide the conversation or mark its host offline.
            return file;
          }
        }),
      );
      if (
        attachments.every((file, index) => file === block.attachments![index])
      )
        return block;
      changed = true;
      return { ...block, attachments };
    }),
  );
  return changed
    ? { ...snapshot, session: { ...snapshot.session, blocks } }
    : snapshot;
}
