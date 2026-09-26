import { readTextFile } from "../../../platform/tauri/fs";
import {
  readTranscriptChunk,
  type TranscriptCursor,
} from "./transcript";

/**
 * The appended text after `offset`, or `""` when there is nothing new.
 *
 * The cursor counts bytes, so the slice has to be taken in bytes too — a
 * transcript with any non-ASCII content would otherwise cut mid-character.
 */
export function chunkAfter(text: string, offset: number): string {
  const bytes = new TextEncoder().encode(text);
  // A file shorter than the cursor was replaced rather than appended to.
  // Rewinding would replay the conversation, so stay put and wait for it to
  // grow past where we already are.
  if (offset >= bytes.length) return "";
  return new TextDecoder().decode(bytes.subarray(offset));
}

export type TranscriptReaderOptions = {
  path: string;
  /** Where the handover left off. Reading from zero replays the conversation. */
  from: TranscriptCursor;
  onRecords: (
    records: Record<string, unknown>[],
    cursor: TranscriptCursor,
  ) => void;
  /** Defaults to the real file read; injected in tests. */
  readFile?: (path: string) => Promise<string>;
  /** A read that failed, including a file that is not there yet. */
  onError?: (error: unknown) => void;
};

export type TranscriptReader = {
  /** One read-and-emit cycle. Safe to call again while a previous call runs. */
  poll: () => Promise<void>;
  cursor: () => TranscriptCursor;
};

export function createTranscriptReader(
  options: TranscriptReaderOptions,
): TranscriptReader {
  const read = options.readFile ?? readTextFile;
  let cursor = options.from;
  let reading = false;

  return {
    cursor: () => cursor,
    poll: async () => {
      // Overlapping reads would both start from the same cursor and emit the
      // same records twice.
      if (reading) return;
      reading = true;
      try {
        // The pty may not have written anything yet, which is ordinary at the
        // moment a session is handed over rather than a failure.
        const text = await read(options.path);
        const chunk = chunkAfter(text, cursor.offset);
        if (!chunk) return;
        const result = readTranscriptChunk(cursor, chunk);
        cursor = result.cursor;
        if (result.records.length > 0) options.onRecords(result.records, cursor);
      } catch (error) {
        options.onError?.(error);
      } finally {
        reading = false;
      }
    },
  };
}

export type TranscriptWatcher = {
  stop: () => void;
  cursor: () => TranscriptCursor;
};

const DEFAULT_INTERVAL_MS = 400;

/**
 * Polls the transcript for appended records until stopped. There is no file
 * watcher in this codebase to follow, and the CLI appends steadily while a turn
 * runs, so an interval is both the local idiom and enough.
 */
export function watchTranscript(
  options: TranscriptReaderOptions & { intervalMs?: number },
): TranscriptWatcher {
  const reader = createTranscriptReader(options);
  let stopped = false;
  const timer = setInterval(() => {
    if (stopped) return;
    void reader.poll();
  }, options.intervalMs ?? DEFAULT_INTERVAL_MS);

  void reader.poll();

  return {
    cursor: reader.cursor,
    stop: () => {
      // Closing twice, or closing an already-archived thread, must be harmless.
      if (stopped) return;
      stopped = true;
      clearInterval(timer);
    },
  };
}
