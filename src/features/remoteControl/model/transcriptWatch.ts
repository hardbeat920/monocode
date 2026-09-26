import { readFileRange, type FileRange } from "../../../platform/tauri/fs";
import {
  readTranscriptChunk,
  type TranscriptCursor,
} from "./transcript";

/** Asked for per read. The command caps this; it is not a promise of size. */
const READ_BYTES = 256 * 1024;

/**
 * Reads allowed in one poll, so a backlog drains in about a second instead of
 * one chunk per tick, without a single poll running unbounded.
 */
const MAX_READS_PER_POLL = 8;

export type TranscriptReaderOptions = {
  path: string;
  /** Where the handover left off. Reading from zero replays the conversation. */
  from: TranscriptCursor;
  onRecords: (
    records: Record<string, unknown>[],
    cursor: TranscriptCursor,
  ) => void;
  /** Defaults to the real ranged read; injected in tests. */
  readRange?: (
    path: string,
    offset: number,
    maxBytes: number,
  ) => Promise<FileRange>;
  /** A read that failed. A file that is not there yet arrives here too. */
  onError?: (error: unknown) => void;
};

export type TranscriptReader = {
  /** One catch-up cycle. Safe to call again while a previous call runs. */
  poll: () => Promise<void>;
  cursor: () => TranscriptCursor;
};

export function createTranscriptReader(
  options: TranscriptReaderOptions,
): TranscriptReader {
  const read = options.readRange ?? readFileRange;
  let cursor = options.from;
  let reading = false;

  return {
    cursor: () => cursor,
    poll: async () => {
      // Overlapping reads would both start from the same cursor and ask for the
      // same bytes.
      if (reading) return;
      reading = true;
      try {
        for (let i = 0; i < MAX_READS_PER_POLL; i += 1) {
          // The pty may not have written anything yet, which is ordinary at the
          // moment a session is handed over rather than a failure. A file
          // shorter than the cursor was replaced rather than appended to, and
          // the command returns nothing for that too: rewinding would replay
          // the conversation, so it waits for the file to grow past us.
          const range = await read(options.path, cursor.offset, READ_BYTES);
          if (!range.text) return;
          const result = readTranscriptChunk(cursor, range.text);
          cursor = result.cursor;
          if (result.records.length > 0) {
            options.onRecords(result.records, cursor);
          }
          if (cursor.offset >= range.size) return;
        }
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
