import { describe, expect, it } from "vitest";
import { attachmentPathText } from "../../../../features/sessions/model/attachments";
import type { Attachment } from "../../../../features/sessions/model/session";
import {
  buildCmdSpawnArgs,
  cmdContentText,
  cmdEventType,
  cmdExitCodeOf,
  cmdExitError,
  cmdFrameType,
  cmdModeArgs,
  cmdPromptWithAttachments,
  cmdRetryDelayMs,
  cmdStatusFromEvent,
  cmdToolTitle,
  CMD_MAX_TURN_ATTEMPTS,
  defaultModelFromCmdListOutput,
  formatCmdErrorMessage,
  formatCmdInlineAttachment,
  isCmdInlineTextAttachment,
  isCmdTransientNetworkError,
  MAX_CMD_INLINE_CHARS,
  modelsFromCmdListOutput,
  parseCmdLine,
  resultStatusFromCmd,
  sessionIdFromCmdLine,
  splitCmdAttachments,
  toolKindFromCmdName,
  toolRefFromCmdEvent,
  truncateCmdDetail,
} from "./cmdProtocol";

const TOOL_QUEUED =
  '{"type":"event","event":{"type":"tool_queued","toolCallId":"call_1","toolName":"read_file","input":{"file_path":"/repo/README.md"}}}';
const TOOL_RUNNING =
  '{"type":"event","event":{"type":"tool_running","toolCallId":"call_1","toolName":"read_file","description":null}}';
const RESULT_SUCCESS =
  '{"type":"result","subtype":"success","sessionId":"abc","stopReason":"end_turn","usage":{},"durationMs":100,"finalText":"ok"}';

describe("cmd protocol", () => {
  it("maps runtime modes to permission flags", () => {
    expect(cmdModeArgs("supervised")).toEqual([
      "--permission-mode",
      "default",
    ]);
    expect(cmdModeArgs("auto-accept-edits")).toEqual(["--yolo"]);
    expect(cmdModeArgs("auto")).toEqual(["--yolo"]);
    expect(cmdModeArgs("full-access")).toEqual(["--yolo"]);
  });

  it("builds one-shot print-mode spawn args", () => {
    expect(
      buildCmdSpawnArgs({ text: "hi", runtimeMode: "full-access" }),
    ).toEqual([
      "-p",
      "hi",
      "--output-format",
      "json",
      "--skip-onboarding",
      "--trust",
      "--yolo",
    ]);
  });

  it("builds plan turns read-only instead of granting runtime permissions", () => {
    const args = buildCmdSpawnArgs({
      text: "plan this",
      runtimeMode: "full-access",
      plan: true,
    });
    expect(args).toContain("--plan");
    expect(args).not.toContain("--yolo");
  });

  it("resumes follow-ups by provider session id", () => {
    const args = buildCmdSpawnArgs({
      text: "again",
      resume: "sess-1",
      model: "deepseek/deepseek-v4-flash",
      effort: "high",
      maxTurns: 10,
      runtimeMode: "supervised",
    });
    expect(args).toContain("--resume");
    expect(args).toContain("sess-1");
    expect(args).toContain("--model");
    expect(args).toContain("--effort");
    expect(args).toContain("--max-turns");
  });

  it("parses frames and skips non-JSON progress text", () => {
    expect(parseCmdLine("  not json  ")).toBeNull();
    const rec = parseCmdLine(TOOL_QUEUED);
    expect(rec).not.toBeNull();
    if (!rec) return;
    expect(cmdFrameType(rec)).toBe("event");
    expect(cmdEventType(rec)).toBe("tool_queued");
  });

  it("reads tool refs from queued/running events", () => {
    const queued = parseCmdLine(TOOL_QUEUED);
    const running = parseCmdLine(TOOL_RUNNING);
    if (!queued || !running) throw new Error("fixtures must parse");
    expect(toolRefFromCmdEvent(queued.event as Record<string, unknown>)).toEqual({
      callId: "call_1",
      name: "read_file",
      input: { file_path: "/repo/README.md" },
    });
    expect(toolRefFromCmdEvent(running.event as Record<string, unknown>)).toEqual({
      callId: "call_1",
      name: "read_file",
      input: {},
    });
  });

  it("binds provider sessions from run_start and result lines", () => {
    const start = parseCmdLine(
      '{"type":"event","event":{"type":"run_start","sessionId":"s-1"}}',
    );
    const result = parseCmdLine(RESULT_SUCCESS);
    if (!start || !result) throw new Error("fixtures must parse");
    expect(sessionIdFromCmdLine(start)).toBe("s-1");
    expect(sessionIdFromCmdLine(result)).toBe("abc");
  });

  it("maps result subtypes to turn outcomes", () => {
    const ok = parseCmdLine(RESULT_SUCCESS);
    if (!ok) throw new Error("fixture must parse");
    expect(resultStatusFromCmd(ok)).toEqual({ status: "completed" });
    expect(
      resultStatusFromCmd({ type: "result", subtype: "max_turns" }),
    ).toEqual({ status: "completed", maxTurns: true });
    expect(
      resultStatusFromCmd({ type: "result", subtype: "error", error: "boom" }),
    ).toEqual({ status: "failed", error: "boom" });
  });

  it("maps exit codes to actionable errors", () => {
    expect(cmdExitError(3, "").message).toMatch(/not authenticated/);
    expect(cmdExitError(10, "").message).toMatch(/credits/);
    expect(cmdExitError(130, "").message).toMatch(/interrupted/);
    expect(cmdExitError(1, "  some failure  ").message).toMatch(/some failure/);
  });

  it("keeps the CLI exit code on errors for retry decisions", () => {
    expect(cmdExitCodeOf(cmdExitError(6, ""))).toBe(6);
    expect(cmdExitCodeOf(cmdExitError(7, ""))).toBe(7);
    expect(cmdExitCodeOf(cmdExitError(3, ""))).toBe(3);
    expect(cmdExitCodeOf(new Error("CommandCode network failure."))).toBeNull();
    expect(cmdExitCodeOf("network failure")).toBeNull();
  });

  it("classifies the CLI's connection failures as transient", () => {
    // The exact print-mode result error behind the interrupted turns.
    expect(
      isCmdTransientNetworkError(
        "Error: Unable to connect to the API. Please check your network connection.",
      ),
    ).toBe(true);
    expect(isCmdTransientNetworkError("CommandCode network failure.")).toBe(true);
    expect(isCmdTransientNetworkError("CommandCode API server error.")).toBe(true);
    expect(isCmdTransientNetworkError("", 6)).toBe(true);
    expect(isCmdTransientNetworkError("", 7)).toBe(true);
    expect(isCmdTransientNetworkError("socket hang up")).toBe(true);
    expect(isCmdTransientNetworkError("other side closed")).toBe(true);
    expect(isCmdTransientNetworkError(cmdExitError(3, "").message)).toBe(false);
    expect(isCmdTransientNetworkError(cmdExitError(5, "").message)).toBe(false);
    expect(isCmdTransientNetworkError("", 3)).toBe(false);
  });

  it("mirrors the CLI's retry backoff envelope", () => {
    expect(CMD_MAX_TURN_ATTEMPTS).toBe(10);
    expect([0, 1, 2, 3].map(cmdRetryDelayMs)).toEqual([
      1_000, 1_000, 1_000, 1_000,
    ]);
    expect([4, 5, 6, 7, 9].map(cmdRetryDelayMs)).toEqual([
      1_600, 3_200, 6_400, 10_000, 10_000,
    ]);
  });

  it("surfaces the CLI's own recovery events as keyed statuses", () => {
    expect(cmdStatusFromEvent({ type: "api_retry", attempt: 3 })).toEqual({
      key: "cmd-connection",
      text: "Connection issue. Retrying… (attempt 3)",
    });
    expect(cmdStatusFromEvent({ type: "api_retry" })).toEqual({
      key: "cmd-connection",
      text: "Connection issue. Retrying…",
    });
    expect(
      cmdStatusFromEvent({ type: "stream_restart", attempt: 1, maxAttempts: 3 }),
    ).toEqual({
      key: "cmd-connection",
      text: "Connection dropped mid-response. Retrying… (1/3)",
    });
    expect(
      cmdStatusFromEvent({
        type: "continuation_recovery",
        attempt: 2,
        maxAttempts: 2,
      }),
    ).toEqual({
      key: "cmd-connection",
      text: "Recovering an interrupted response (2/2).",
    });
    expect(cmdStatusFromEvent({ type: "text_delta", delta: "x" })).toBeNull();
  });

  it("classifies cmd tool names", () => {
    expect(toolKindFromCmdName("read_file")).toBe("read");
    expect(toolKindFromCmdName("write_file")).toBe("edit");
    expect(toolKindFromCmdName("shell_command")).toBe("shell");
    expect(toolKindFromCmdName("grep")).toBe("search");
    expect(toolKindFromCmdName("agent")).toBe("agent");
    expect(toolKindFromCmdName("todo_write")).toBe("task");
    expect(toolKindFromCmdName("ask_user_question")).toBe("question");
  });

  it("titles tools from their inputs", () => {
    expect(
      cmdToolTitle("read_file", { file_path: "/repo/README.md" }),
    ).toMatch(/README/);
    expect(cmdToolTitle("grep", { pattern: "hello" })).toMatch(/hello/);
  });

  it("parses cmd --list-models output", () => {
    const output = [
      "Available models  ·  3 models",
      "",
      "Open Source",
      "",
      "deepseek/deepseek-v4-flash               fast hybrid-attention reasoning (default)",
      "moonshotai/kimi-k3                       long-horizon coding",
      "meta/muse-spark-1.3-contributor        Muse Spark 1.3 at up to 95% off",
      "not-a-model-line",
    ].join("\n");
    const models = modelsFromCmdListOutput(output);
    expect(models.map((model) => model.nativeId)).toEqual([
      "deepseek/deepseek-v4-flash",
      "moonshotai/kimi-k3",
      "meta/muse-spark-1.3-contributor",
    ]);
    expect(models[0]).toMatchObject({ id: "cmd:deepseek/deepseek-v4-flash", harness: "cmd" });
    expect(defaultModelFromCmdListOutput(output)).toBe(
      "deepseek/deepseek-v4-flash",
    );
  });

  it("attaches per-model effort levels with xhigh default for muse 1.3 contributor", () => {
    const output = [
      "meta/muse-spark-1.3-contributor        Muse Spark 1.3 at up to 95% off",
      "deepseek/deepseek-v4-flash             fast hybrid-attention reasoning",
      "minimaxai/minimax-m3                   frontier coding",
      "z-ai/glm-5.3-flash                     fast, affordable GLM coding",
      "zai-org/glm-5.2-fast                   high-throughput GLM-5.2",
      "some-org/unknown-model                 something new",
    ].join("\n");
    const models = modelsFromCmdListOutput(output);
    const effort = (nativeId: string) =>
      models
        .find((model) => model.nativeId === nativeId)
        ?.settings?.find((setting) => setting.id === "effort");
    expect(effort("meta/muse-spark-1.3-contributor")).toMatchObject({
      value: "xhigh",
      options: [
        { value: "low", label: "Low" },
        { value: "medium", label: "Medium" },
        { value: "high", label: "High" },
        { value: "xhigh", label: "Extra High" },
      ],
    });
    expect(
      effort("deepseek/deepseek-v4-flash")?.options.map((option) => option.value),
    ).toEqual(["off", "high", "max"]);
    expect(
      effort("minimaxai/minimax-m3")?.options.map((option) => option.value),
    ).toEqual(["low", "medium", "high"]);
    expect(
      effort("z-ai/glm-5.3-flash")?.options.map((option) => option.value),
    ).toEqual(["low", "high", "max"]);
    expect(effort("z-ai/glm-5.3-flash")).toMatchObject({
      value: "high",
      options: [
        { value: "low", label: "Low" },
        { value: "high", label: "High" },
        { value: "max", label: "Max" },
      ],
    });
    // GLM-5.2 Fast has no adjustable reasoning effort: no effort setting.
    expect(effort("zai-org/glm-5.2-fast")).toBeUndefined();
    // Unknown ids get no Reasoning control rather than an unsupported level.
    expect(effort("some-org/unknown-model")).toBeUndefined();
  });

  it("parses bare provider model ids and stops before decision models", () => {
    const output = [
      "Available models  ·  4 models",
      "",
      "Anthropic",
      "",
      "claude-sonnet-5-5            best combo of speed & intelligence (recommended)",
      "claude-haiku-4-5             fastest & most compact",
      "",
      "OpenAI",
      "",
      "gpt-6-astra                  most capable OpenAI model",
      "",
      "Decision models (headless only)",
      "typesafe/jev  typed questions in, probabilities out",
    ].join("\n");
    const models = modelsFromCmdListOutput(output);
    expect(models.map((model) => model.nativeId)).toEqual([
      "claude-sonnet-5-5",
      "claude-haiku-4-5",
      "gpt-6-astra",
    ]);
    expect(models[0]?.settings).toBeDefined();
    expect(models[1]?.settings).toBeUndefined();
    expect(defaultModelFromCmdListOutput(output)).toBeNull();
  });

  it("keeps tool output text readable in transcript rows", () => {
    expect(
      cmdContentText([
        { type: "text", text: "line one\n" },
        { type: "tool_use", id: "x" },
        { type: "text", text: "line two" },
      ]),
    ).toBe("line one\nline two");
    expect(truncateCmdDetail("  short  ")).toBe("short");
    expect(truncateCmdDetail("abcdef", 3)).toContain("truncated 3 chars");
  });

  it("appends attachments as path lines in the print-mode prompt", () => {
    const attachments: Attachment[] = [
      {
        id: "a",
        name: "report.pdf",
        mimeType: "application/pdf",
        kind: "file",
        size: 100,
        path: "/tmp/report.pdf",
      },
    ];
    expect(cmdPromptWithAttachments({ text: "hi", attachments })).toBe(
      `hi\n\n${attachmentPathText(attachments[0] as Attachment)}`,
    );
    expect(cmdPromptWithAttachments({ text: "hi" })).toBe("hi");
    const args = buildCmdSpawnArgs({
      text: "hi",
      attachments,
      runtimeMode: "supervised",
    });
    expect(args[1]).toContain("Attached file");
  });

  it("sends pasted images as exact-read path references, never base64", () => {

    // cmd -p has no vision channel (verified against the CLI: markdown and
    // @-mention forms also resolve through a read_file tool call), so the
    // prompt must stay a short path reference and keep base64 out of argv.
    const data = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==";
    const attachments: Attachment[] = [
      {
        id: "img",
        name: "red.png",
        mimeType: "image/png",
        kind: "image",
        size: 95,
        path: "/tmp/monocode-attachments/red.png",
        data,
      },
    ];
    const prompt = cmdPromptWithAttachments({ text: "look", attachments });
    expect(prompt).toContain('"/tmp/monocode-attachments/red.png"');
    expect(prompt).toContain("Read this exact file only");
    expect(prompt).not.toContain("base64");
    expect(prompt).not.toContain(data);
    const args = buildCmdSpawnArgs({
      text: "look",
      attachments,
      runtimeMode: "supervised",
    });
    expect(args.join("\n")).not.toContain(data);
  });

  it("inlines markdown/text attachments so no file part is created", () => {
    const md: Attachment = {
      id: "m",
      name: "API.md",
      mimeType: "text/markdown",
      kind: "file",
      size: 100,
      path: "/tmp/API.md",
    };
    const pdf: Attachment = {
      id: "p",
      name: "report.pdf",
      mimeType: "application/pdf",
      kind: "file",
      size: 100,
      path: "/tmp/report.pdf",
    };
    const img: Attachment = {
      id: "i",
      name: "shot.png",
      mimeType: "image/png",
      kind: "image",
      size: 100,
      path: "/tmp/shot.png",
    };
    expect(isCmdInlineTextAttachment(md)).toBe(true);
    expect(isCmdInlineTextAttachment(pdf)).toBe(false);
    expect(isCmdInlineTextAttachment(img)).toBe(false);
    expect(
      isCmdInlineTextAttachment({ ...md, path: undefined }),
    ).toBe(false);
    const split = splitCmdAttachments([md, pdf, img]);
    expect(split.inline.map((a) => a.id)).toEqual(["m"]);
    expect(split.referenced.map((a) => a.id)).toEqual(["p", "i"]);
  });

  it("formats inlined markdown as a fenced block", () => {
    const block = formatCmdInlineAttachment("API.md", "text/markdown", "# API\nbody\n");
    expect(block).toContain('Attached file "API.md"');
    expect(block).toContain("do not send this as a file part");
    expect(block).toContain("```md\n# API\nbody\n```");
  });

  it("escapes fences and truncates oversized inline content", () => {
    const tricky = "a\n```\ncode\n```\nb";
    const block = formatCmdInlineAttachment("n.md", "text/markdown", tricky);
    expect(block).toContain("````md\n");
    expect(block).toContain("\n````");
    expect(block).not.toMatch(/\n```md\n/);
    const big = "x".repeat(MAX_CMD_INLINE_CHARS + 10);
    const cut = formatCmdInlineAttachment("big.md", "text/markdown", big);
    expect(cut).toContain("truncated 10 chars");
    expect(cut.length).toBeLessThan(big.length + 500);
  });

  it("translates CLI-surfaced backend failures into actionable errors", () => {
    expect(
      formatCmdErrorMessage(
        "'file part media type text/markdown' functionality not supported.",
      ),
    ).toContain("doesn't accept text/markdown file attachments");
    expect(
      formatCmdErrorMessage(
        "Failed to read OpenCode response: response too big for into-string",
      ),
    ).toContain("too large");
    expect(
      formatCmdErrorMessage(
        "Error: Unable to connect to the API. Please check your network connection.",
      ),
    ).toBe(
      "CommandCode network error: Unable to connect to the API. Please check your network connection.",
    );
  });
});
