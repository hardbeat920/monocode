/**
 * The transcript mirror, tested against real records.
 *
 * Every fixture line below is **verbatim** from a throwaway session this
 * investigation drove under a pty — `claude --remote-control` in a scratch
 * directory. Hand-written fixtures agree with whatever the implementation
 * assumes; real ones disagree when the assumption is wrong, which is the only
 * reason to keep them. They were selected down to what the tests need and are
 * otherwise untouched, `usage` blocks and all. Where a test needs a shape the
 * CLI did not produce, it says so and edits a copy.
 *
 * What they are, in the order the CLI wrote them:
 *   - `bridge_status` from a process start, and a second one from a later
 *     `--resume` of the same conversation
 *   - a multi-line user message (bracketed paste, submitted as one message)
 *   - an assistant text reply, and the `turn_duration` that closed its turn
 *   - a `Write` tool call, its `tool_result`, and the reply after it
 *   - a user message the CLI queued because a turn was already running
 *   - the last user message in the file, whose turn was interrupted with ESC:
 *     nothing follows it, which is the case §8 of the design doc is about
 */

import { describe, expect, it } from "vitest";

import {
  bridgeStatusFromRecords,
  createMirrorState,
  emptyCursor,
  mapRecord,
  readTranscriptChunk,
  resolveTurnFromScreen,
  type MirrorEvent,
} from "./transcript";

const BRIDGE_STATUS =
  "{\"parentUuid\":null,\"isSidechain\":false,\"type\":\"system\",\"subtype\":\"bridge_status\",\"content\":\"/remote-control is active \u00b7 Continue here, on your phone, or at https://claude.ai/code/session_01P9pJfVXWNcrT2zAP3NHShE\",\"url\":\"https://claude.ai/code/session_01P9pJfVXWNcrT2zAP3NHShE\",\"isMeta\":false,\"timestamp\":\"2026-09-26T08:51:30.852Z\",\"uuid\":\"41e69752-4e26-4289-8cca-b07daff0008a\",\"userType\":\"external\",\"entrypoint\":\"cli\",\"cwd\":\"/private/tmp/claude-501/-Users-medeni-Desktop-Projects-monocode/bfba8763-531d-4557-ae2d-e7004d663f72/scratchpad/rctest\",\"sessionId\":\"ce433f3f-f049-4760-a4b5-f470d4e1648b\",\"version\":\"2.1.221\",\"gitBranch\":\"HEAD\"}";
const USER_MULTILINE =
  "{\"parentUuid\":\"41e69752-4e26-4289-8cca-b07daff0008a\",\"isSidechain\":false,\"promptId\":\"b8d17e8e-7b36-46f7-933a-6b3dd0d5e0f5\",\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"Two things, on separate lines:\\nline one: say ALPHA\\nline two: say BETA\"},\"uuid\":\"64a2498f-f7ae-4eb4-bce1-f891ecb178fb\",\"timestamp\":\"2026-09-26T08:51:38.639Z\",\"permissionMode\":\"default\",\"origin\":{\"kind\":\"human\"},\"promptSource\":\"typed\",\"userType\":\"external\",\"entrypoint\":\"cli\",\"cwd\":\"/private/tmp/claude-501/-Users-medeni-Desktop-Projects-monocode/bfba8763-531d-4557-ae2d-e7004d663f72/scratchpad/rctest\",\"sessionId\":\"ce433f3f-f049-4760-a4b5-f470d4e1648b\",\"version\":\"2.1.221\",\"gitBranch\":\"HEAD\"}";
const ASSISTANT_TEXT =
  "{\"parentUuid\":\"afe1dce6-9fbd-45ff-8681-8720b94f5cd0\",\"isSidechain\":false,\"message\":{\"model\":\"claude-opus-5\",\"id\":\"msg_011CfRmNy19Trgsc5h6rSqkK\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"ALPHA\\nBETA\"}],\"container\":null,\"stop_reason\":\"end_turn\",\"stop_sequence\":null,\"stop_details\":null,\"usage\":{\"input_tokens\":2,\"cache_creation_input_tokens\":43689,\"cache_read_input_tokens\":0,\"output_tokens\":11,\"server_tool_use\":{\"web_search_requests\":0,\"web_fetch_requests\":0},\"service_tier\":\"standard\",\"cache_creation\":{\"ephemeral_1h_input_tokens\":43689,\"ephemeral_5m_input_tokens\":0},\"inference_geo\":\"not_available\",\"iterations\":[{\"input_tokens\":2,\"output_tokens\":11,\"cache_read_input_tokens\":0,\"cache_creation_input_tokens\":43689,\"cache_creation\":{\"ephemeral_5m_input_tokens\":0,\"ephemeral_1h_input_tokens\":43689},\"type\":\"message\"}],\"speed\":\"standard\"},\"diagnostics\":null,\"context_management\":null},\"requestId\":\"req_011CfRmNxZc8R3T4fMMofrtr\",\"type\":\"assistant\",\"uuid\":\"8204a1cf-0b0c-4520-afd7-520e214ea52a\",\"timestamp\":\"2026-09-26T08:51:41.561Z\",\"effort\":\"high\",\"session_id\":\"ce433f3f-f049-4760-a4b5-f470d4e1648b\",\"userType\":\"external\",\"entrypoint\":\"cli\",\"cwd\":\"/private/tmp/claude-501/-Users-medeni-Desktop-Projects-monocode/bfba8763-531d-4557-ae2d-e7004d663f72/scratchpad/rctest\",\"sessionId\":\"ce433f3f-f049-4760-a4b5-f470d4e1648b\",\"version\":\"2.1.221\",\"gitBranch\":\"HEAD\"}";
const TURN_DURATION =
  "{\"parentUuid\":\"25cd6062-bc09-448e-82f2-a916c3be55b6\",\"isSidechain\":false,\"type\":\"system\",\"subtype\":\"turn_duration\",\"durationMs\":2923,\"messageCount\":9,\"timestamp\":\"2026-09-26T08:51:41.764Z\",\"uuid\":\"a764472f-27d8-40eb-8171-04b917ad60c9\",\"isMeta\":false,\"userType\":\"external\",\"entrypoint\":\"cli\",\"cwd\":\"/private/tmp/claude-501/-Users-medeni-Desktop-Projects-monocode/bfba8763-531d-4557-ae2d-e7004d663f72/scratchpad/rctest\",\"sessionId\":\"ce433f3f-f049-4760-a4b5-f470d4e1648b\",\"version\":\"2.1.221\",\"gitBranch\":\"HEAD\"}";
const USER_WRITE_REQUEST =
  "{\"parentUuid\":\"a764472f-27d8-40eb-8171-04b917ad60c9\",\"isSidechain\":false,\"promptId\":\"0ac806b4-bb0e-4e55-bcf3-f33ef92bedef\",\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"Use the Write tool to create a file named probe.txt containing the word hello.\"},\"uuid\":\"86b55ae6-ec95-4df3-85a5-b6624de3e8c4\",\"timestamp\":\"2026-09-26T08:52:34.267Z\",\"permissionMode\":\"default\",\"origin\":{\"kind\":\"human\"},\"promptSource\":\"typed\",\"userType\":\"external\",\"entrypoint\":\"cli\",\"cwd\":\"/private/tmp/claude-501/-Users-medeni-Desktop-Projects-monocode/bfba8763-531d-4557-ae2d-e7004d663f72/scratchpad/rctest\",\"sessionId\":\"ce433f3f-f049-4760-a4b5-f470d4e1648b\",\"version\":\"2.1.221\",\"gitBranch\":\"HEAD\"}";
const ASSISTANT_TOOL_USE =
  "{\"parentUuid\":\"9005334b-75a4-4899-bcd6-cd07f25e24ed\",\"isSidechain\":false,\"message\":{\"model\":\"claude-opus-5\",\"id\":\"msg_011CfRmT51QeG6HiMh4VmmdN\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"tool_use\",\"id\":\"toolu_01PwZALcQukAxqrwjkP91FEk\",\"name\":\"Write\",\"input\":{\"file_path\":\"/private/tmp/claude-501/-Users-medeni-Desktop-Projects-monocode/bfba8763-531d-4557-ae2d-e7004d663f72/scratchpad/rctest/probe.txt\",\"content\":\"hello\\n\"},\"caller\":{\"type\":\"direct\"}}],\"container\":null,\"stop_reason\":\"tool_use\",\"stop_sequence\":null,\"stop_details\":null,\"usage\":{\"input_tokens\":2,\"cache_creation_input_tokens\":61,\"cache_read_input_tokens\":43689,\"output_tokens\":143,\"server_tool_use\":{\"web_search_requests\":0,\"web_fetch_requests\":0},\"service_tier\":\"standard\",\"cache_creation\":{\"ephemeral_1h_input_tokens\":61,\"ephemeral_5m_input_tokens\":0},\"inference_geo\":\"not_available\",\"iterations\":[{\"input_tokens\":2,\"output_tokens\":143,\"cache_read_input_tokens\":43689,\"cache_creation_input_tokens\":61,\"cache_creation\":{\"ephemeral_5m_input_tokens\":0,\"ephemeral_1h_input_tokens\":61},\"type\":\"message\"}],\"speed\":\"standard\"},\"diagnostics\":null,\"context_management\":null},\"requestId\":\"req_011CfRmT4WeLd21DHfjSamK8\",\"type\":\"assistant\",\"uuid\":\"0902b0a1-1a3c-4d3e-b7ff-e75b1ae7ce5b\",\"timestamp\":\"2026-09-26T08:52:38.206Z\",\"effort\":\"high\",\"session_id\":\"ce433f3f-f049-4760-a4b5-f470d4e1648b\",\"userType\":\"external\",\"entrypoint\":\"cli\",\"cwd\":\"/private/tmp/claude-501/-Users-medeni-Desktop-Projects-monocode/bfba8763-531d-4557-ae2d-e7004d663f72/scratchpad/rctest\",\"sessionId\":\"ce433f3f-f049-4760-a4b5-f470d4e1648b\",\"version\":\"2.1.221\",\"gitBranch\":\"HEAD\"}";
const USER_TOOL_RESULT =
  "{\"parentUuid\":\"0902b0a1-1a3c-4d3e-b7ff-e75b1ae7ce5b\",\"isSidechain\":false,\"promptId\":\"0ac806b4-bb0e-4e55-bcf3-f33ef92bedef\",\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"tool_use_id\":\"toolu_01PwZALcQukAxqrwjkP91FEk\",\"type\":\"tool_result\",\"content\":\"File created successfully at: /private/tmp/claude-501/-Users-medeni-Desktop-Projects-monocode/bfba8763-531d-4557-ae2d-e7004d663f72/scratchpad/rctest/probe.txt (file state is current in your context \u2014 no need to Read it back)\"}]},\"uuid\":\"62de5eb5-385f-4ea5-b5be-f49767a39b68\",\"timestamp\":\"2026-09-26T08:53:14.568Z\",\"toolUseResult\":{\"type\":\"create\",\"filePath\":\"/private/tmp/claude-501/-Users-medeni-Desktop-Projects-monocode/bfba8763-531d-4557-ae2d-e7004d663f72/scratchpad/rctest/probe.txt\",\"content\":\"hello\\n\",\"structuredPatch\":[],\"originalFile\":null,\"userModified\":false},\"sourceToolAssistantUUID\":\"0902b0a1-1a3c-4d3e-b7ff-e75b1ae7ce5b\",\"session_id\":\"ce433f3f-f049-4760-a4b5-f470d4e1648b\",\"userType\":\"external\",\"entrypoint\":\"cli\",\"cwd\":\"/private/tmp/claude-501/-Users-medeni-Desktop-Projects-monocode/bfba8763-531d-4557-ae2d-e7004d663f72/scratchpad/rctest\",\"sessionId\":\"ce433f3f-f049-4760-a4b5-f470d4e1648b\",\"version\":\"2.1.221\",\"gitBranch\":\"HEAD\"}";
const ASSISTANT_AFTER_TOOL =
  "{\"parentUuid\":\"4fed8da4-9888-496e-b632-d8455d3db5de\",\"isSidechain\":false,\"message\":{\"model\":\"claude-opus-5\",\"id\":\"msg_011CfRmW2MWQttrfigjbGXiB\",\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"Created `probe.txt` in the working directory, containing `hello`.\"}],\"container\":null,\"stop_reason\":\"end_turn\",\"stop_sequence\":null,\"stop_details\":null,\"usage\":{\"input_tokens\":2,\"cache_creation_input_tokens\":269,\"cache_read_input_tokens\":43750,\"output_tokens\":24,\"server_tool_use\":{\"web_search_requests\":0,\"web_fetch_requests\":0},\"service_tier\":\"standard\",\"cache_creation\":{\"ephemeral_1h_input_tokens\":269,\"ephemeral_5m_input_tokens\":0},\"inference_geo\":\"not_available\",\"iterations\":[{\"input_tokens\":2,\"output_tokens\":24,\"cache_read_input_tokens\":43750,\"cache_creation_input_tokens\":269,\"cache_creation\":{\"ephemeral_5m_input_tokens\":0,\"ephemeral_1h_input_tokens\":269},\"type\":\"message\"}],\"speed\":\"standard\"},\"diagnostics\":null,\"context_management\":null},\"requestId\":\"req_011CfRmW1vTWMdFoSXy5gGRU\",\"type\":\"assistant\",\"uuid\":\"15fa330f-f9d2-4c79-92ef-b65652e640fd\",\"timestamp\":\"2026-09-26T08:53:16.733Z\",\"effort\":\"high\",\"session_id\":\"ce433f3f-f049-4760-a4b5-f470d4e1648b\",\"userType\":\"external\",\"entrypoint\":\"cli\",\"cwd\":\"/private/tmp/claude-501/-Users-medeni-Desktop-Projects-monocode/bfba8763-531d-4557-ae2d-e7004d663f72/scratchpad/rctest\",\"sessionId\":\"ce433f3f-f049-4760-a4b5-f470d4e1648b\",\"version\":\"2.1.221\",\"gitBranch\":\"HEAD\"}";
const TURN_DURATION_2 =
  "{\"parentUuid\":\"f04efeb5-dfcd-47a4-ba4c-18dd1844e97f\",\"isSidechain\":false,\"type\":\"system\",\"subtype\":\"turn_duration\",\"durationMs\":6390,\"messageCount\":17,\"timestamp\":\"2026-09-26T08:53:16.933Z\",\"uuid\":\"403f5711-ce98-433a-b464-d6a6505e86e8\",\"isMeta\":false,\"userType\":\"external\",\"entrypoint\":\"cli\",\"cwd\":\"/private/tmp/claude-501/-Users-medeni-Desktop-Projects-monocode/bfba8763-531d-4557-ae2d-e7004d663f72/scratchpad/rctest\",\"sessionId\":\"ce433f3f-f049-4760-a4b5-f470d4e1648b\",\"version\":\"2.1.221\",\"gitBranch\":\"HEAD\"}";
const BRIDGE_STATUS_2 =
  "{\"parentUuid\":\"403f5711-ce98-433a-b464-d6a6505e86e8\",\"isSidechain\":false,\"type\":\"system\",\"subtype\":\"bridge_status\",\"content\":\"/remote-control is active \u00b7 Continue here, on your phone, or at https://claude.ai/code/session_01P9pJfVXWNcrT2zAP3NHShE\",\"url\":\"https://claude.ai/code/session_01P9pJfVXWNcrT2zAP3NHShE\",\"isMeta\":false,\"timestamp\":\"2026-09-26T08:54:48.219Z\",\"uuid\":\"37a65b40-e72e-4f8e-8607-df5b392c07d6\",\"userType\":\"external\",\"entrypoint\":\"cli\",\"cwd\":\"/private/tmp/claude-501/-Users-medeni-Desktop-Projects-monocode/bfba8763-531d-4557-ae2d-e7004d663f72/scratchpad/rctest\",\"sessionId\":\"ce433f3f-f049-4760-a4b5-f470d4e1648b\",\"version\":\"2.1.221\",\"gitBranch\":\"HEAD\"}";
const USER_QUEUED =
  "{\"parentUuid\":\"c7af7592-0f08-4dbd-986f-d955419a1a9e\",\"isSidechain\":false,\"promptId\":\"f4565cb8-62a3-4149-beea-4375ff9e351c\",\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"second message sent while busy: say GAMMA\"},\"uuid\":\"2d26313b-f18f-493e-9124-390aa15e54a7\",\"timestamp\":\"2026-09-26T08:55:02.801Z\",\"permissionMode\":\"default\",\"origin\":{\"kind\":\"human\"},\"promptSource\":\"queued\",\"userType\":\"external\",\"entrypoint\":\"cli\",\"cwd\":\"/private/tmp/claude-501/-Users-medeni-Desktop-Projects-monocode/bfba8763-531d-4557-ae2d-e7004d663f72/scratchpad/rctest\",\"sessionId\":\"ce433f3f-f049-4760-a4b5-f470d4e1648b\",\"version\":\"2.1.221\",\"gitBranch\":\"HEAD\"}";
const USER_INTERRUPTED =
  "{\"parentUuid\":\"ae10beb3-3f1d-402c-a7c6-f6cc95880571\",\"isSidechain\":false,\"promptId\":\"a823ab28-4737-4c3e-ae6b-11acc95a3859\",\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"Write a very long essay about the history of the abacus, at least 2000 words. Think carefully first.\"},\"uuid\":\"988a6312-7179-4ffb-9a98-54c1a9b488f9\",\"timestamp\":\"2026-09-26T08:58:54.632Z\",\"permissionMode\":\"default\",\"origin\":{\"kind\":\"human\"},\"promptSource\":\"typed\",\"userType\":\"external\",\"entrypoint\":\"cli\",\"cwd\":\"/private/tmp/claude-501/-Users-medeni-Desktop-Projects-monocode/bfba8763-531d-4557-ae2d-e7004d663f72/scratchpad/rctest\",\"sessionId\":\"ce433f3f-f049-4760-a4b5-f470d4e1648b\",\"version\":\"2.1.221\",\"gitBranch\":\"HEAD\"}";

const FIXTURE =
  [
    BRIDGE_STATUS,
    USER_MULTILINE,
    ASSISTANT_TEXT,
    TURN_DURATION,
    USER_WRITE_REQUEST,
    ASSISTANT_TOOL_USE,
    USER_TOOL_RESULT,
    ASSISTANT_AFTER_TOOL,
    TURN_DURATION_2,
    BRIDGE_STATUS_2,
    USER_QUEUED,
    USER_INTERRUPTED,
  ].join("\n") + "\n";

/** The bridge url this conversation was given, unchanged by every resume. */
const BRIDGE_URL = "https://claude.ai/code/session_01P9pJfVXWNcrT2zAP3NHShE";

function bytes(text: string): number {
  return new TextEncoder().encode(text).length;
}

function recordsOf(...jsonl: string[]): Record<string, unknown>[] {
  return readTranscriptChunk(emptyCursor(), jsonl.join("\n") + "\n").records;
}

function eventsOf(...jsonl: string[]): MirrorEvent[] {
  const state = createMirrorState();
  return recordsOf(...jsonl).flatMap((rec) => mapRecord(state, rec));
}

function types(events: MirrorEvent[]): string[] {
  return events.map((event) => event.type);
}

describe("readTranscriptChunk", () => {
  it("reads whole lines and reports the new offset", () => {
    const read = readTranscriptChunk(emptyCursor(), FIXTURE);
    expect(read.records).toHaveLength(12);
    expect(read.skipped).toBe(0);
    expect(read.cursor.partial).toBe("");
    expect(read.cursor.offset).toBe(bytes(FIXTURE));
  });

  it("starts from the offset it was given, not from zero", () => {
    // Handover captures the file's length, so the mirror never replays history
    // the UI already has.
    const read = readTranscriptChunk(emptyCursor(9000), ASSISTANT_TEXT + "\n");
    expect(read.cursor.offset).toBe(9000 + bytes(ASSISTANT_TEXT + "\n"));
  });

  it("buffers a half-written trailing line and completes it on the next chunk", () => {
    // The file is appended to while it is read, so a chunk boundary lands
    // mid-record far more often than it lands on a newline.
    const split = Math.floor(FIXTURE.length / 2);
    const first = readTranscriptChunk(emptyCursor(), FIXTURE.slice(0, split));
    expect(first.cursor.partial).not.toBe("");
    expect(first.skipped).toBe(0);

    const second = readTranscriptChunk(first.cursor, FIXTURE.slice(split));
    expect(first.records.length + second.records.length).toBe(12);
    expect(second.skipped).toBe(0);
    expect(second.cursor.partial).toBe("");
    // The buffered bytes were counted once, not twice.
    expect(second.cursor.offset).toBe(bytes(FIXTURE));
  });

  it("reassembles a line split inside a multi-byte character", () => {
    // bridge_status carries a "·", so this proves the line is rejoined as text.
    const whole = BRIDGE_STATUS + "\n";
    const at = whole.indexOf("·");
    expect(at).toBeGreaterThan(0);
    const first = readTranscriptChunk(emptyCursor(), whole.slice(0, at));
    const second = readTranscriptChunk(first.cursor, whole.slice(at));
    expect(second.records).toHaveLength(1);
    expect(bridgeStatusFromRecords(second.records).url).toBe(BRIDGE_URL);
    expect(second.cursor.offset).toBe(bytes(whole));
  });

  it("counts a malformed line and keeps reading the ones behind it", () => {
    const read = readTranscriptChunk(
      emptyCursor(),
      [ASSISTANT_TEXT, "{not json at all", TURN_DURATION].join("\n") + "\n",
    );
    expect(read.skipped).toBe(1);
    expect(read.records).toHaveLength(2);
  });

  it("ignores blank lines without counting them as damage", () => {
    const read = readTranscriptChunk(emptyCursor(), "\n" + ASSISTANT_TEXT + "\n\n");
    expect(read.records).toHaveLength(1);
    expect(read.skipped).toBe(0);
  });
});

describe("bridgeStatusFromRecords", () => {
  it("takes the url verbatim and appends nothing to it", () => {
    const status = bridgeStatusFromRecords(recordsOf(BRIDGE_STATUS));
    expect(status.active).toBe(true);
    expect(status.url).toBe(BRIDGE_URL);
    // The "/rc" on screen is the TUI's status-line indicator, not a suffix.
    expect(status.url?.endsWith("/rc")).toBe(false);
  });

  it("reports inactive when the conversation has no bridge_status record", () => {
    const status = bridgeStatusFromRecords(
      recordsOf(USER_MULTILINE, ASSISTANT_TEXT, TURN_DURATION),
    );
    expect(status).toEqual({ active: false });
  });

  it("keeps one url when a resume writes a second record", () => {
    // Measured: this conversation was resumed three times and kept the same
    // bridge id every time, so a second record must not read as a second link.
    const status = bridgeStatusFromRecords(
      recordsOf(BRIDGE_STATUS, ASSISTANT_TEXT, BRIDGE_STATUS_2),
    );
    expect(status).toEqual({ active: true, url: BRIDGE_URL });
  });

  it("follows the newer record if a restart ever does hand out a new url", () => {
    // Not observed — every resume reused the id — so this is an edited copy,
    // covering the case rather than claiming it happens.
    const moved = BRIDGE_STATUS_2.replaceAll(
      "session_01P9pJfVXWNcrT2zAP3NHShE",
      "session_01NEWaaaaaaaaaaaaaaaaaaa",
    );
    const status = bridgeStatusFromRecords(recordsOf(BRIDGE_STATUS, moved));
    expect(status.url).toBe(
      "https://claude.ai/code/session_01NEWaaaaaaaaaaaaaaaaaaa",
    );
  });

  it("falls back to the content line when the url field is missing", () => {
    // This is CLI output, not an API; the message text is the backstop.
    const withoutUrl = JSON.parse(BRIDGE_STATUS) as Record<string, unknown>;
    delete withoutUrl.url;
    const status = bridgeStatusFromRecords([withoutUrl]);
    expect(status.url).toBe(BRIDGE_URL);
  });

  it("is recorded on mirror state as records go by", () => {
    const state = createMirrorState();
    expect(state.bridge.active).toBe(false);
    for (const rec of recordsOf(BRIDGE_STATUS)) mapRecord(state, rec);
    expect(state.bridge).toEqual({ active: true, url: BRIDGE_URL });
  });
});

describe("mapRecord", () => {
  it("surfaces an inbound user message with its newlines intact", () => {
    const events = eventsOf(USER_MULTILINE);
    expect(types(events)).toEqual(["remote.userMessage"]);
    const message = events[0];
    if (message.type !== "remote.userMessage") throw new Error("shape");
    expect(message.text).toBe(
      "Two things, on separate lines:\nline one: say ALPHA\nline two: say BETA",
    );
    expect(message.promptSource).toBe("typed");
    expect(message.originKind).toBe("human");
  });

  it("passes an unrecognised promptSource through instead of dropping it", () => {
    // A phone-originated turn has never been observed, so its promptSource is
    // unknown. Filtering for "typed" would make the phone invisible, which is
    // the one failure that would make the whole feature look broken.
    const events = eventsOf(
      USER_QUEUED.replace('"promptSource":"queued"', '"promptSource":"telepathy"'),
    );
    const message = events[0];
    if (message?.type !== "remote.userMessage") throw new Error("shape");
    expect(message.promptSource).toBe("telepathy");
    expect(message.text).toBe("second message sent while busy: say GAMMA");
  });

  it("still surfaces a user message carrying no origin metadata at all", () => {
    const bare = JSON.stringify({
      type: "user",
      message: { role: "user", content: "from somewhere unknown" },
    });
    expect(types(eventsOf(bare))).toEqual(["remote.userMessage"]);
  });

  it("emits an assistant reply as one delta, because there is no token stream", () => {
    const events = eventsOf(ASSISTANT_TEXT);
    expect(types(events)).toEqual(["message.delta", "context"]);
    const delta = events[0];
    if (delta.type !== "message.delta") throw new Error("shape");
    expect(delta.text).toBe("ALPHA\nBETA");
  });

  it("emits nothing for a thinking block, because the text is never persisted", () => {
    // Measured: all 14334 thinking blocks written by an interactive CLI 2.1.2xx
    // on this machine had an empty `thinking` string, keeping only the
    // signature. This is the shape the CLI actually writes.
    const redacted = JSON.stringify({
      type: "assistant",
      message: {
        role: "assistant",
        content: [{ type: "thinking", thinking: "", signature: "c2ln" }],
      },
    });
    expect(types(eventsOf(redacted))).toEqual([]);
  });

  it("mirrors reasoning if a future CLI ever does persist it", () => {
    const withText = JSON.stringify({
      type: "assistant",
      message: {
        role: "assistant",
        content: [
          { type: "thinking", thinking: "weighing it up", signature: "c2ln" },
        ],
      },
    });
    expect(types(eventsOf(withText))).toEqual(["reasoning.delta"]);
  });

  it("pairs a tool call with its result through the whole sequence", () => {
    const events = eventsOf(
      USER_WRITE_REQUEST,
      ASSISTANT_TOOL_USE,
      USER_TOOL_RESULT,
      ASSISTANT_AFTER_TOOL,
      TURN_DURATION_2,
    );
    expect(types(events)).toEqual([
      "remote.userMessage",
      "tool.started",
      "context",
      "tool.updated",
      "message.delta",
      "context",
      "message.completed",
    ]);

    const started = events[1];
    if (started.type !== "tool.started") throw new Error("shape");
    expect(started.kind).toBe("edit");
    // `toolTitle` returns the tool's name, the same as on the headless path —
    // the file is carried by the preview, which is where the UI reads it from.
    expect(started.title).toBe("Write");
    expect(started.preview?.path).toContain("probe.txt");

    const updated = events[3];
    if (updated.type !== "tool.updated") throw new Error("shape");
    // Matched to its call by tool_use_id, so it carries that call's title.
    expect(updated.callId).toBe(started.callId);
    expect(updated.status).toBe("completed");
    expect(updated.detail).toContain("File created successfully");
  });

  it("treats a result with no is_error field as a success", () => {
    // 7322 of the 26225 tool_result blocks on this machine omit `is_error`
    // entirely, this fixture's among them. Absent must not read as failed.
    const events = eventsOf(ASSISTANT_TOOL_USE, USER_TOOL_RESULT);
    const updated = events.find((event) => event.type === "tool.updated");
    if (updated?.type !== "tool.updated") throw new Error("shape");
    expect(updated.status).toBe("completed");
  });

  it("marks a failed tool result as failed", () => {
    // Built by editing the parsed record, not by string replacement: the field
    // is missing here, so a replace would match nothing and quietly assert the
    // success case twice.
    const failed = JSON.parse(USER_TOOL_RESULT) as Record<string, unknown>;
    const message = failed.message as { content: Record<string, unknown>[] };
    message.content[0].is_error = true;
    const state = createMirrorState();
    const events = [JSON.parse(ASSISTANT_TOOL_USE), failed].flatMap((rec) =>
      mapRecord(state, rec as Record<string, unknown>),
    );
    const updated = events.find((event) => event.type === "tool.updated");
    if (updated?.type !== "tool.updated") throw new Error("shape");
    expect(updated.status).toBe("failed");
  });

  it("ignores a tool result whose call it never saw", () => {
    // A mirror that attaches mid-turn has no record of the call, and inventing
    // a row for it would render a tool with no name.
    expect(types(eventsOf(USER_TOOL_RESULT))).toEqual([]);
  });

  it("closes the open message when turn_duration ends the turn", () => {
    expect(types(eventsOf(USER_MULTILINE, ASSISTANT_TEXT, TURN_DURATION))).toEqual([
      "remote.userMessage",
      "message.delta",
      "context",
      "message.completed",
    ]);
  });

  it("skips subagent traffic rather than mixing it into the thread", () => {
    // `isSidechain` was false on all 23628 user and assistant records on this
    // machine, and `parent_tool_use_id` never appears in a transcript at all —
    // subagent messages evidently go to their own files. The guard is here for
    // if that changes, so the test drives it rather than describing reality.
    const sidechain = JSON.parse(ASSISTANT_TEXT) as Record<string, unknown>;
    expect(sidechain.isSidechain).toBe(false);
    sidechain.isSidechain = true;
    const state = createMirrorState();
    expect(mapRecord(state, sidechain)).toEqual([]);
  });

  it("ignores record types the mirror has no use for", () => {
    const events = eventsOf(
      JSON.stringify({ type: "file-history-snapshot", snapshot: {} }),
      JSON.stringify({ type: "queue-operation", operation: "enqueue" }),
      JSON.stringify({ type: "ai-title", aiTitle: "something" }),
    );
    expect(events).toEqual([]);
  });
});

describe("inbound messages that are not just text", () => {
  // The shape is verbatim from the corpus — every one of the 137 image blocks
  // across this machine's 161 transcripts is
  // {type:"image", source:{type:"base64", media_type, data}} — but the payload
  // here is a 1x1 PNG generated for the test. The real ones are the user's
  // screenshots and have no business in a repo.
  const PNG =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";
  const image = (mediaType = "image/png") => ({
    type: "image",
    source: { type: "base64", media_type: mediaType, data: PNG },
  });
  const userRecord = (content: unknown) =>
    JSON.stringify({
      type: "user",
      uuid: "1f0e3dad-99a0-4a6f-b0d1-4d4a5a5f5a5a",
      promptSource: "typed",
      origin: { kind: "human" },
      message: { role: "user", content },
    });

  it("surfaces an image-only message instead of dropping it", () => {
    // 55 user records on this machine produced no event at all before this,
    // every one an image with no caption — so the assistant's reply would
    // appear with nothing to have prompted it.
    const events = eventsOf(userRecord([image()]));
    expect(types(events)).toEqual(["remote.userMessage"]);
    const message = events[0];
    if (message.type !== "remote.userMessage") throw new Error("shape");
    expect(message.text).toBe("");
    expect(message.attachments).toEqual([
      { blockType: "image", mediaType: "image/png", bytes: 69, index: 0 },
    ]);
    // Still a real user turn, so it opens one.
    expect(message.promptSource).toBe("typed");
  });

  it("describes an attachment without carrying its bytes", () => {
    // Images arrive base64 inline and the corpus holds 23MB of them, so an
    // event that inlined the payload would charge every record for a minority.
    const events = eventsOf(userRecord([image("image/jpeg")]));
    expect(JSON.stringify(events)).not.toContain(PNG.slice(0, 40));
    const message = events[0];
    if (message?.type !== "remote.userMessage") throw new Error("shape");
    expect(message.attachments?.[0].mediaType).toBe("image/jpeg");
    // 92 base64 chars with one "=" of padding decode to 69 bytes.
    expect(message.attachments?.[0].bytes).toBe(69);
  });

  it("keeps the text and the images of a mixed message", () => {
    // 25 messages in the corpus mix the two; their images used to vanish while
    // the text showed, which reads as a complete message and is not one.
    const events = eventsOf(
      userRecord([{ type: "text", text: "what is wrong here?" }, image()]),
    );
    const message = events[0];
    if (message?.type !== "remote.userMessage") throw new Error("shape");
    expect(message.text).toBe("what is wrong here?");
    expect(message.attachments).toHaveLength(1);
    expect(message.attachments?.[0].index).toBe(1);
  });

  it("indexes every attachment so the caller can re-read it", () => {
    // The corpus has a message with six images; the index plus the record uuid
    // is what locates each block in the session file.
    const events = eventsOf(userRecord([image(), image(), image()]));
    const message = events[0];
    if (message?.type !== "remote.userMessage") throw new Error("shape");
    expect(message.attachments?.map((a) => a.index)).toEqual([0, 1, 2]);
    expect(message.uuid).toBe("1f0e3dad-99a0-4a6f-b0d1-4d4a5a5f5a5a");
  });

  it("surfaces a block type nobody has seen yet rather than dropping it", () => {
    // blockType is kept verbatim for the same reason promptSource is: the cost
    // of not recognising something must never be that it disappears.
    const events = eventsOf(userRecord([{ type: "document", id: "doc_1" }]));
    const message = events[0];
    if (message?.type !== "remote.userMessage") throw new Error("shape");
    expect(message.attachments).toEqual([{ blockType: "document", index: 0 }]);
  });

  it("omits attachments entirely for a text-only message", () => {
    const events = eventsOf(USER_MULTILINE);
    const message = events[0];
    if (message?.type !== "remote.userMessage") throw new Error("shape");
    expect(message.attachments).toBeUndefined();
  });

  it("still emits nothing when there is genuinely no content", () => {
    expect(types(eventsOf(userRecord([])))).toEqual([]);
    expect(types(eventsOf(userRecord("")))).toEqual([]);
  });

  it("opens a turn for an image-only message, as for any other", () => {
    const state = createMirrorState();
    for (const rec of recordsOf(userRecord([image()]))) mapRecord(state, rec);
    expect(state.turn).toEqual({ active: true, source: "record" });
  });
});

describe("turn state when no record ever arrives", () => {
  it("leaves an interrupted turn resolvable instead of pending forever", () => {
    // ESC writes nothing — not even turn_duration — so the file simply stops
    // after the user record. The transcript alone can never end this turn.
    const state = createMirrorState();
    const events = recordsOf(USER_INTERRUPTED).flatMap((rec) =>
      mapRecord(state, rec),
    );
    expect(types(events)).toEqual(["remote.userMessage"]);
    expect(state.turn).toEqual({ active: true, source: "record" });

    // The screen going idle is the only remaining evidence, and it is enough.
    expect(resolveTurnFromScreen(state, true)).toEqual([]);
    expect(state.turn).toEqual({ active: false, source: "screen" });
  });

  it("closes a half-emitted message when the screen goes idle", () => {
    const state = createMirrorState();
    for (const rec of recordsOf(USER_INTERRUPTED, ASSISTANT_TEXT)) {
      mapRecord(state, rec);
    }
    expect(types(resolveTurnFromScreen(state, true))).toEqual([
      "message.completed",
    ]);
  });

  it("does nothing while the screen is still busy", () => {
    const state = createMirrorState();
    for (const rec of recordsOf(USER_INTERRUPTED)) mapRecord(state, rec);
    expect(resolveTurnFromScreen(state, false)).toEqual([]);
    expect(state.turn.active).toBe(true);
  });

  it("is idempotent once the turn is already over", () => {
    const state = createMirrorState();
    for (const rec of recordsOf(USER_MULTILINE, ASSISTANT_TEXT, TURN_DURATION)) {
      mapRecord(state, rec);
    }
    expect(state.turn.active).toBe(false);
    expect(resolveTurnFromScreen(state, true)).toEqual([]);
  });
});
