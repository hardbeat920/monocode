/**
 * Reading a pty-hosted Claude session from its transcript file.
 *
 * When Remote Control owns a conversation, MonoCode's headless child is gone
 * and with it the `stream-json` feed the UI is built on. The interactive CLI
 * still writes every message to `~/.claude/projects/<encoded cwd>/<id>.jsonl`
 * as the turn progresses, so that file becomes the inbound half of the mirror.
 * See `docs/remote-control.md` §3–§4.
 *
 * This module is the parsing half only: bytes in, `HarnessEvent`s out. It does
 * no IO, holds no timers and touches no React, so the same record sequence
 * always produces the same events. Opening the file, watching it grow and
 * deciding when to poll belong to the caller — which is also what keeps this
 * testable without a CLI or a pty.
 *
 * Events use `HarnessEvent` rather than a parallel vocabulary, so a mirrored
 * turn renders through exactly the same UI path as a headless one.
 */

import type { HarnessEvent } from "../../../integrations/harness/core/types";
import {
  asRecord,
  assistantTextBlocks,
  assistantThinkingBlocks,
  assistantToolUses,
  parseJsonLine,
  statusTextFromSystem,
  previewFromTool,
  stringField,
  toolKindFromName,
  toolResultsFromUserMessage,
  toolTitle,
} from "../../../integrations/harness/providers/claude/claudeProtocol";

/**
 * A message somebody else sent into this conversation.
 *
 * `HarnessEvent` has no variant for this, and that is not an oversight in it:
 * on the headless path MonoCode *is* the sender, so the UI already holds the
 * text before the provider echoes it, and the `user` records in the stream only
 * ever carry tool results — which is why `handleUser` in `claude.ts` looks at
 * nothing else. A mirror inverts that: a turn can arrive from the phone or from
 * the TUI, and the UI has never seen it. `interjection` is the closest existing
 * event and is wrong — it appends a *system* block. So the mirror emits this
 * alongside harness events and the caller decides how to seat it.
 */
/**
 * A non-text block on an inbound user message — an image today.
 *
 * The bytes are deliberately **not** carried. Images arrive base64 inline, and
 * on this machine 137 of them total 23MB, median 148KB and one 625KB; a message
 * can hold six. Putting that in an event would charge every record for a
 * minority case. What is here is enough to decide and to fetch: the caller
 * already knows the session file, so the message's `uuid` plus this `index`
 * locates the block to re-read.
 */
export type RemoteAttachment = {
  /**
   * The block's own `type`, verbatim — `"image"` is the only one observed, and
   * keeping it unnormalised means a block type nobody has seen yet still
   * surfaces instead of vanishing, which is the whole point of this event.
   */
  blockType: string;
  /** e.g. `"image/jpeg"`. Absent when the record did not say. */
  mediaType?: string;
  /** Decoded payload size, so a caller can budget before reading the bytes. */
  bytes?: number;
  /** Position within `message.content`. */
  index: number;
};

export type RemoteUserMessage = {
  type: "remote.userMessage";
  text: string;
  /** Omitted when the message is text only. */
  attachments?: RemoteAttachment[];
  /** Verbatim, unvalidated. See `mapUser` for why it is not filtered. */
  promptSource?: string;
  originKind?: string;
  uuid?: string;
};

export type MirrorEvent = HarnessEvent | RemoteUserMessage;

/** Where the last read stopped, plus any half-written line it ended on. */
export type TranscriptCursor = {
  /** Byte offset in the session file. The next read starts here. */
  offset: number;
  /**
   * A trailing line the file did not finish yet. Its bytes are counted in
   * `offset`, so it is held here rather than re-read.
   */
  partial: string;
};

export type TranscriptRead = {
  cursor: TranscriptCursor;
  records: Record<string, unknown>[];
  /** Lines that were complete but never valid JSON. Counted, not thrown. */
  skipped: number;
};

export function emptyCursor(offset = 0): TranscriptCursor {
  return { offset, partial: "" };
}

/**
 * Split a chunk read from `cursor.offset` into records.
 *
 * The file is appended to while it is read, so the chunk routinely ends
 * mid-line. That tail is buffered into the returned cursor and completed by the
 * next chunk; dropping it would lose a message and throwing would end the
 * mirror on a timing accident. A line that is complete but unparseable is
 * counted in `skipped` and passed over — one corrupt line must not stop the
 * stream behind it.
 *
 * `chunk` must be decoded with a streaming decoder (`TextDecoder` with
 * `{ stream: true }`), or read whole: a multi-byte character split across two
 * reads would otherwise arrive as a replacement character and cost us the line.
 */
export function readTranscriptChunk(
  cursor: TranscriptCursor,
  chunk: string,
): TranscriptRead {
  const text = cursor.partial + chunk;
  const lines = text.split("\n");
  // Without a trailing newline the last element is still being written.
  const partial = lines.pop() ?? "";
  const records: Record<string, unknown>[] = [];
  let skipped = 0;
  for (const line of lines) {
    if (!line.trim()) continue;
    const rec = parseJsonLine(line);
    if (rec) records.push(rec);
    else skipped += 1;
  }
  return {
    cursor: { offset: cursor.offset + byteLength(chunk), partial },
    records,
    skipped,
  };
}

function byteLength(text: string): number {
  return new TextEncoder().encode(text).length;
}

/**
 * Whether Remote Control is active for this conversation, and where to reach
 * it. One `bridge_status` record is written per process start, so its presence
 * is the signal and a later one supersedes an earlier one.
 */
export type BridgeStatus = {
  active: boolean;
  url?: string;
};

/**
 * The URL is taken from the record's own `url` field. **Nothing is appended to
 * it** — the `/rc` that looks like a suffix on screen is the TUI's status-line
 * indicator painted 114 columns and 28 rows away, not part of the link
 * (`docs/remote-control.md` §6). `content` is a fallback only, because this is
 * CLI output and its shape is not an API.
 */
export function bridgeStatusFromRecords(
  records: Record<string, unknown>[],
  previous: BridgeStatus = { active: false },
): BridgeStatus {
  let status = previous;
  for (const rec of records) {
    if (!isBridgeStatus(rec)) continue;
    const url =
      stringField(rec, "url") ??
      urlFromText(stringField(rec, "content") ?? "") ??
      status.url;
    status = url ? { active: true, url } : { active: true };
  }
  return status;
}

function isBridgeStatus(rec: Record<string, unknown>): boolean {
  return (
    stringField(rec, "type") === "system" &&
    stringField(rec, "subtype") === "bridge_status"
  );
}

function urlFromText(text: string): string | undefined {
  const match = /https:\/\/\S*claude\.ai\/code\/session_[A-Za-z0-9]+/.exec(text);
  return match?.[0];
}

/**
 * Turn state as the transcript can know it.
 *
 * A user record opens a turn and `system`/`subtype:"turn_duration"` closes one.
 * An **interrupt closes neither**: ESC leaves no record at all, so a mirror that
 * waits for `turn_duration` would show a turn running forever
 * (`docs/remote-control.md` §8). This module therefore never claims a turn is
 * still running on the strength of a missing record — it reports what it saw
 * and exposes `resolveTurnFromScreen` so the caller, which watches the pty, can
 * end a turn the file never ended.
 */
export type TurnState = {
  active: boolean;
  /** How the current value was reached, so a caller can tell silence apart. */
  source: "none" | "record" | "screen";
};

export type MirrorState = {
  turn: TurnState;
  /** Tool calls seen in this mirror, so results can be matched to their call. */
  tools: Map<string, { name: string; input: Record<string, unknown>; title: string }>;
  /**
   * Calls whose `tool_result` has not arrived. Unlike `tools`, entries are
   * removed, because this answers a live question: a tool is running right now.
   * That is the one thing the transcript can say about liveness, and it is what
   * keeps a silence rule from mistaking a slow tool for an interrupt (§8).
   */
  outstandingTools: Set<string>;
  /** An assistant message is open and its UI block is not closed yet. */
  pendingAssistant: boolean;
  pendingReasoning: boolean;
  bridge: BridgeStatus;
};

export function createMirrorState(): MirrorState {
  return {
    turn: { active: false, source: "none" },
    tools: new Map(),
    outstandingTools: new Set(),
    pendingAssistant: false,
    pendingReasoning: false,
    bridge: { active: false },
  };
}

/**
 * Map one transcript record onto the events MonoCode already renders.
 *
 * `state` is updated in place, the way `Live` is on the headless path. Nothing
 * here reads a clock or the filesystem, so a record sequence is reproducible.
 */
export function mapRecord(
  state: MirrorState,
  rec: Record<string, unknown>,
): MirrorEvent[] {
  const type = stringField(rec, "type");
  if (type === "system") return mapSystem(state, rec);
  if (isSidechainRecord(rec)) return [];
  if (type === "assistant") return mapAssistant(state, rec);
  if (type === "user") return mapUser(state, rec);
  return [];
}

/**
 * Subagent traffic, which belongs on its parent Agent row rather than loose in
 * the thread.
 *
 * Deliberately not `isSubagentMessage` from the protocol module: that keys off
 * `parent_tool_use_id`, which the `stream-json` feed carries and a transcript
 * record never does — reusing it here would be a guard that never fires. Both
 * markers are checked because neither has been observed in a session file at
 * all: `isSidechain` is present on every user and assistant record and was
 * `false` on all 23628 of them, so subagent messages evidently live in their own
 * files. This is a cheap guard against that changing, not a described case.
 */
function isSidechainRecord(rec: Record<string, unknown>): boolean {
  if (rec.isSidechain === true) return true;
  const parent = rec.parent_tool_use_id;
  return typeof parent === "string" && parent.length > 0;
}

function mapSystem(
  state: MirrorState,
  rec: Record<string, unknown>,
): MirrorEvent[] {
  if (isBridgeStatus(rec)) {
    state.bridge = bridgeStatusFromRecords([rec], state.bridge);
    return [];
  }
  const subtype = stringField(rec, "subtype") ?? "";
  if (subtype.endsWith("compact_boundary")) return mapCompaction(rec, subtype);
  if (subtype !== "turn_duration") return [];
  return endTurn(state, "record");
}

/**
 * A compaction, which rewrites the conversation the mirror is reading.
 *
 * Worth surfacing rather than ignoring: the context level moves by a factor —
 * one record in the corpus went from 672003 tokens to 11301 — so a UI that
 * missed it would keep showing the pre-compaction reading indefinitely.
 *
 * `statusTextFromSystem` is reused so the row reads identically to the headless
 * path, which already emits a `status` for these. Note it tests
 * `subtype.startsWith("compact")`, so it answers for `compact_boundary` and not
 * for `microcompact_boundary`; the fallback covers that rather than leaving the
 * smaller cousin silent.
 */
function mapCompaction(
  rec: Record<string, unknown>,
  subtype: string,
): MirrorEvent[] {
  const events: MirrorEvent[] = [
    { type: "status", text: statusTextFromSystem(rec) ?? "Compacted context" },
  ];
  // Only `compact_boundary` states the level it landed on. `microcompact`
  // reports `preTokens` and `tokensSaved`, and their difference is arithmetic
  // rather than a reading, so no context event is emitted for it.
  const used = numberField(
    asRecord(rec.compactMetadata) ?? {},
    "postTokens",
  );
  if (subtype === "compact_boundary" && used > 0) {
    events.push({ type: "context", used });
  }
  return events;
}

function mapAssistant(
  state: MirrorState,
  rec: Record<string, unknown>,
): MirrorEvent[] {
  const events: MirrorEvent[] = [];

  // Reasoning is emitted only when the block actually carries text. On this
  // machine it never does: all 14334 `thinking` blocks written by an
  // interactive CLI 2.1.2xx had an empty `thinking` string and kept only the
  // signature. Handling it anyway costs nothing and means a CLI that starts
  // persisting reasoning is mirrored without a change here.
  const thinking = assistantThinkingBlocks(rec).join("");
  if (thinking) {
    events.push({ type: "reasoning.delta", text: thinking });
    state.pendingReasoning = true;
  }
  if (state.pendingReasoning && !thinking) {
    state.pendingReasoning = false;
    events.push({ type: "reasoning.completed" });
  }

  const text = assistantTextBlocks(rec).join("");
  if (text) {
    // The transcript has no token stream: one whole message arrives at once, so
    // it is emitted as a single delta and closed when the next message starts.
    if (state.pendingAssistant) events.push({ type: "message.completed" });
    events.push({ type: "message.delta", text });
    state.pendingAssistant = true;
  }

  for (const use of assistantToolUses(rec)) {
    const title = toolTitle(use.name, use.input);
    state.tools.set(use.id, { name: use.name, input: use.input, title });
    state.outstandingTools.add(use.id);
    events.push({
      type: "tool.started",
      callId: use.id,
      title,
      kind: toolKindFromName(use.name),
      status: "pending",
      preview: previewFromTool(use.name, use.input),
    });
  }

  const used = contextUsedFromAssistant(rec);
  if (used !== undefined) events.push({ type: "context", used });
  return events;
}

function mapUser(
  state: MirrorState,
  rec: Record<string, unknown>,
): MirrorEvent[] {
  const results = toolResultsFromUserMessage(rec);
  if (results.length > 0) {
    const events: MirrorEvent[] = [];
    for (const result of results) {
      state.outstandingTools.delete(result.toolUseId);
      const tool = state.tools.get(result.toolUseId);
      if (!tool) continue;
      events.push({
        type: "tool.updated",
        callId: result.toolUseId,
        title: tool.title,
        kind: toolKindFromName(tool.name),
        status: result.isError ? "failed" : "completed",
        detail: result.text || undefined,
        preview: previewFromTool(tool.name, tool.input, result.text),
      });
    }
    return events;
  }

  const text = userText(rec);
  const attachments = userAttachments(rec);
  // Any user record that is not a tool result is somebody sending a message,
  // whoever they are. `promptSource` and `origin.kind` are deliberately not
  // filtered: a phone-originated turn has never been observed and its values
  // are unknown, so an unrecognised one must still appear (§12).
  //
  // The same rule is why one block of *any* kind is enough to emit. Requiring
  // text used to drop 55 messages across this machine's 161 transcripts, every
  // one of them an image with no caption: the assistant's reply would appear in
  // MonoCode with nothing to have prompted it.
  if (!text && attachments.length === 0) return [];
  const events: MirrorEvent[] = [];
  if (state.pendingAssistant) {
    state.pendingAssistant = false;
    events.push({ type: "message.completed" });
  }
  const promptSource = stringField(rec, "promptSource");
  const originKind = stringField(asRecord(rec.origin), "kind");
  events.push({
    type: "remote.userMessage",
    text,
    ...(attachments.length > 0 ? { attachments } : {}),
    ...(promptSource ? { promptSource } : {}),
    ...(originKind ? { originKind } : {}),
    ...(stringField(rec, "uuid") ? { uuid: stringField(rec, "uuid") } : {}),
  });
  state.turn = { active: true, source: "record" };
  return events;
}

/** Every block that is not text, described rather than carried. */
function userAttachments(rec: Record<string, unknown>): RemoteAttachment[] {
  const content = asRecord(rec.message)?.content;
  if (!Array.isArray(content)) return [];
  const out: RemoteAttachment[] = [];
  content.forEach((block, index) => {
    const row = asRecord(block);
    const blockType = stringField(row, "type");
    if (!row || !blockType || blockType === "text") return;
    const source = asRecord(row.source);
    const mediaType = stringField(source, "media_type");
    const data = source?.data;
    out.push({
      blockType,
      ...(mediaType ? { mediaType } : {}),
      ...(typeof data === "string" ? { bytes: decodedSize(data) } : {}),
      index,
    });
  });
  return out;
}

/** Bytes a base64 payload decodes to, without decoding it. */
function decodedSize(base64: string): number {
  const padding = base64.endsWith("==") ? 2 : base64.endsWith("=") ? 1 : 0;
  return Math.max(0, Math.floor((base64.length * 3) / 4) - padding);
}

function userText(rec: Record<string, unknown>): string {
  const content = asRecord(rec.message)?.content;
  if (typeof content === "string") return content;
  if (!Array.isArray(content)) return "";
  return content
    .flatMap((block) => {
      const row = asRecord(block);
      if (stringField(row, "type") !== "text") return [];
      return typeof row?.text === "string" ? [row.text] : [];
    })
    .join("");
}

/**
 * What the caller must observe for an interrupted turn to resolve.
 *
 * The transcript cannot do this alone, and not for want of looking: an
 * interrupted turn and a user typing ahead mid-turn produce **byte-identical**
 * transcript state — an outstanding `user` record with no terminator behind it.
 * Both were measured. An interrupt left a session file unchanged for 240s, and
 * unsubmitted type-ahead during a live turn wrote nothing for 148s before the
 * turn completed normally. So "is a user record still outstanding" is true in
 * both cases and discriminates neither.
 *
 * What does separate them is whether the pty is still producing output, which
 * only the caller can see:
 *
 * | | live turn | interrupted |
 * | --- | --- | --- |
 * | longest gap between pty reads | **0.80s** (278 reads, thinking and streaming) | indefinite — 90s and 291s measured |
 * | output rate | 6556 bytes/s streaming, ~740 bytes/s thinking | stops within 0.1s of the ESC |
 *
 * Hence `quietForMs`. A threshold around 3000ms sits well clear of the 800ms
 * worst case observed while a turn was genuinely alive.
 */
export type TurnLiveness = {
  /** Milliseconds since the pty last produced any output. */
  quietForMs: number;
  /**
   * The screen shows a composer holding text — `composerHeld`. After an
   * interrupt the CLI restores the interrupted prompt there, so this is the
   * positive trace. On its own it is ambiguous with type-ahead, which is why
   * `quietForMs` is also required.
   */
  composerHeld: boolean;
  /** Milliseconds of silence that count as "not running". Defaults to 3000. */
  quietThresholdMs?: number;
};

const DEFAULT_QUIET_MS = 3000;

/**
 * End a turn on evidence from outside the transcript.
 *
 * Accepts the old boolean — `true` still means "the screen says this turn is
 * over, take my word for it" — or a {@link TurnLiveness} reading, which is the
 * form that can resolve an interrupt safely.
 *
 * A tool still in flight vetoes the silence rule. A turn waiting on a slow
 * command is genuinely alive while producing nothing, and the transcript is the
 * only thing that knows a `tool_use` has no `tool_result` yet, so that check
 * belongs here rather than in the caller.
 */
export function resolveTurnFromScreen(
  state: MirrorState,
  signal: boolean | TurnLiveness,
): MirrorEvent[] {
  if (typeof signal === "boolean") {
    return signal ? endTurn(state, "screen") : [];
  }
  if (!signal.composerHeld) return [];
  if (state.outstandingTools.size > 0) return [];
  const threshold = signal.quietThresholdMs ?? DEFAULT_QUIET_MS;
  if (signal.quietForMs < threshold) return [];
  return endTurn(state, "screen");
}

function endTurn(
  state: MirrorState,
  source: "record" | "screen",
): MirrorEvent[] {
  const events: MirrorEvent[] = [];
  if (state.pendingReasoning) {
    state.pendingReasoning = false;
    events.push({ type: "reasoning.completed" });
  }
  if (state.pendingAssistant) {
    state.pendingAssistant = false;
    events.push({ type: "message.completed" });
  }
  if (!state.turn.active && events.length === 0) return [];
  state.turn = { active: false, source };
  return events;
}

function contextUsedFromAssistant(
  rec: Record<string, unknown>,
): number | undefined {
  const usage = asRecord(asRecord(rec.message)?.usage);
  if (!usage) return undefined;
  const total =
    numberField(usage, "input_tokens") +
    numberField(usage, "cache_read_input_tokens") +
    numberField(usage, "cache_creation_input_tokens");
  return total > 0 ? total : undefined;
}

function numberField(rec: Record<string, unknown>, key: string): number {
  const value = rec[key];
  return typeof value === "number" && Number.isFinite(value) ? value : 0;
}
