import { describe, expect, it } from "vitest";
import {
  MuseExecFold,
  museApprovalArgs,
  museAuthError,
  museEffort,
  museEffortForModel,
  musePromptParts,
  museSpawnArgs,
  toolTitleForTaskKind,
} from "./museProtocol";

const SID = "01a0cfef-7b4a-7a21-8da4-98066fcadbd7";

function rawLine(record: unknown): string {
  return JSON.stringify(record);
}

function line(payloadType: string, payload: unknown): string {
  return JSON.stringify({
    schema_version: 1,
    stream: { kind: "session", id: SID },
    payload_type: payloadType,
    payload,
  });
}

describe("muse spawn args", () => {
  it("starts a fresh session without a resume id", () => {
    expect(
      museSpawnArgs({
        cwd: "/tmp/work",
        prompt: "hi",
        images: [],
        runtimeMode: "supervised",
      }),
    ).toEqual(["exec", "--json", "--workspace", "/tmp/work", "hi"]);
  });

  it("resumes with session id, model, effort and images", () => {
    expect(
      museSpawnArgs({
        cwd: "/tmp/work",
        prompt: "hi",
        images: ["/tmp/work/shot.png"],
        resumeSessionId: SID,
        modelId: "muse-spark-1.3",
        effort: "low",
        runtimeMode: "supervised",
      }),
    ).toEqual([
      "exec",
      "--json",
      "--workspace",
      "/tmp/work",
      "--session-id",
      SID,
      "--model",
      "muse-spark-1.3",
      "--reasoning-effort",
      "low",
      "--image",
      "/tmp/work/shot.png",
      "hi",
    ]);
  });

  it("relaxes approvals only for full-access, never the sandbox", () => {
    expect(museApprovalArgs("supervised")).toEqual([]);
    expect(museApprovalArgs("auto")).toEqual([]);
    expect(museApprovalArgs("full-access")).toEqual(["--approval-mode", "never"]);
    const args = museSpawnArgs({
      cwd: "/tmp/work",
      prompt: "hi",
      images: [],
      runtimeMode: "full-access",
    });
    expect(args).toContain("--approval-mode");
    expect(args.join(" ")).not.toMatch(/--yolo|--disable-sandbox|--disable-approval/);
  });

  it("reads effort from model settings and rejects unknown tiers", () => {
    expect(museEffort({ effort: "xhigh" })).toBe("xhigh");
    expect(museEffort({ reasoning: "low" })).toBe("low");
    expect(museEffort({ effort: "turbo" })).toBeUndefined();
    expect(museEffort(undefined)).toBeUndefined();
  });

  it("drops undocumented tiers and contributor-unavailable max", () => {
    // `ultra` is not a documented tier and `none` returns HTTP 400 on Spark.
    expect(museEffort({ effort: "ultra" })).toBeUndefined();
    expect(museEffort({ effort: "none" })).toBeUndefined();
    // `max` exists only on Standard-tier Muse Spark.
    expect(museEffortForModel({ effort: "max" }, "muse-spark-1.3")).toBe("max");
    expect(museEffortForModel({ effort: "max" }, "muse-spark-1.3-contributor")).toBeUndefined();
    expect(museEffortForModel({ effort: "high" }, "muse-spark-1.3-contributor")).toBe("high");
  });

  it("notes pathless attachments in the prompt text", () => {
    const parts = musePromptParts(
      "look",
      [
        {
          id: "c",
          name: "pasted.png",
          mimeType: "image/png",
          kind: "image",
          size: 10,
        },
      ],
      "/tmp/work",
    );
    expect(parts.images).toEqual([]);
    expect(parts.text).toContain("[Note: 1 attachment(s) could not be attached");
    expect(parts.text).toContain("pasted.png");
  });

  it("routes image attachments to --image and files to mentions", () => {
    const parts = musePromptParts(
      "look",
      [
        {
          id: "a",
          name: "shot.png",
          mimeType: "image/png",
          kind: "image",
          size: 10,
          path: "/tmp/work/shot.png",
        },
        {
          id: "b",
          name: "notes.txt",
          mimeType: "text/plain",
          kind: "file",
          size: 10,
          path: "/tmp/work/notes.txt",
        },
      ],
      "/tmp/work",
    );
    expect(parts.images).toEqual(["/tmp/work/shot.png"]);
    expect(parts.text).toContain("@notes.txt");
  });
});

describe("muse auth errors", () => {
  it("recognizes login failures", () => {
    expect(museAuthError("not logged in, run `muse login`")).toBe(true);
    expect(museAuthError("turn complete")).toBe(false);
  });
});

describe("muse exec fold", () => {
  it("captures the session id and streams deltas to completion", () => {
    const fold = new MuseExecFold();
    expect(fold.sessionId).toBeUndefined();
    fold.pushLine(line("session.run.linked", { kind: "session_run_linked" }));
    expect(fold.sessionId).toBe(SID);
    const deltas = fold.pushLine(line("run.output.delta", { kind: "run_output_delta", text: "hi" }));
    expect(deltas).toEqual([{ type: "message.delta", text: "hi" }]);
    const end = fold.pushLine(
      line("run.terminal.completed", { kind: "run_terminal", terminal: "completed" }),
    );
    expect(fold.done).toBe(true);
    expect(end).toEqual([{ type: "message.completed" }]);
  });

  it("maps tool tasks to started/updated events with paths", () => {
    const fold = new MuseExecFold();
    const taskId = "01a0cfef-ed4c-7953-bf2e-4f8a751c9db0";
    const callId = "call_01a0cfefeaae77e2a1b5f62448f06eb8";
    const started = fold.pushLine(
      line("task.lifecycle.proposed", {
        kind: "task_lifecycle",
        task_id: taskId,
        event: { kind: "proposed", task_id: taskId, task_kind: "tool.write_file" },
      }),
    );
    expect(started).toEqual([{ type: "tool.started", callId: taskId, title: "write_file" }]);
    // Repeat lifecycle events must not duplicate the started event.
    expect(
      fold.pushLine(
        line("task.lifecycle.started", {
          kind: "task_lifecycle",
          task_id: taskId,
          event: { kind: "started", task_id: taskId },
        }),
      ),
    ).toEqual([]);
    const updated = fold.pushLine(
      line("task.lifecycle.output", {
        kind: "task_lifecycle",
        task_id: taskId,
        event: { kind: "output", task_id: taskId, chunk: "wrote 8 bytes", final_result: true },
      }),
    );
    expect(updated).toEqual([{ type: "tool.updated", callId: taskId, detail: "wrote 8 bytes" }]);
    fold.pushLine(
      line("task.lifecycle.tool_output_ref", {
        kind: "task_lifecycle",
        task_id: taskId,
        event: {
          kind: "tool_output_ref",
          task_id: taskId,
          output_ref: { id: `tool_patch-${taskId}-${callId}` },
        },
      }),
    );
    const result = fold.pushLine(
      line("tool.result", {
        kind: "tool_result",
        call_id: callId,
        text: "wrote 8 bytes to /tmp/muse-probe.txt",
        edit_facts: { tool_name: "write_file", path: "/tmp/muse-probe.txt", added: 1 },
        correlation_facts: { tool_name: "write_file", outcome: "success" },
      }),
    );
    expect(result).toEqual([
      {
        type: "tool.updated",
        callId: taskId,
        title: "write_file",
        status: "completed",
        detail: "wrote 8 bytes to /tmp/muse-probe.txt",
        paths: ["/tmp/muse-probe.txt"],
      },
    ]);
  });

  it("ignores model and reminder tasks, garbage and unknown payloads", () => {
    const fold = new MuseExecFold();
    expect(fold.pushLine("not json")).toEqual([]);
    expect(
      fold.pushLine(
        line("task.lifecycle.proposed", {
          kind: "task_lifecycle",
          task_id: "t1",
          event: { kind: "proposed", task_id: "t1", task_kind: "model.meta.response" },
        }),
      ),
    ).toEqual([]);
    expect(fold.pushLine(line("session.something.new", { kind: "x" }))).toEqual([]);
    expect(fold.done).toBe(false);
  });

  it("reports non-completed terminals as errors", () => {
    const fold = new MuseExecFold();
    const events = fold.pushLine(
      line("run.terminal.failed", { kind: "run_terminal", terminal: "failed", reason: "boom" }),
    );
    expect(fold.done).toBe(true);
    expect(events[0]).toEqual({ type: "message.completed" });
    expect(events[1]).toEqual({ type: "session.error", message: "Muse turn ended (failed): boom" });
  });

  it("titles tool kinds", () => {
    expect(toolTitleForTaskKind("tool.write_file")).toBe("write_file");
  });

  it("only adopts ids from the session stream", () => {
    const fold = new MuseExecFold();
    fold.pushLine(
      rawLine({
        stream: { kind: "run", id: "run-id-not-a-session" },
        payload_type: "run.output.delta",
        payload: { kind: "run_output_delta", text: "x" },
      }),
    );
    expect(fold.sessionId).toBeUndefined();
    fold.pushLine(
      rawLine({
        stream: { kind: "session", id: SID },
        payload_type: "run.output.delta",
        payload: { kind: "run_output_delta", text: "x" },
      }),
    );
    expect(fold.sessionId).toBe(SID);
  });

  it("joins tool results with dashed call ids", () => {
    const fold = new MuseExecFold();
    const taskId = "task-1";
    const callId = "call_abc-123_def";
    fold.pushLine(
      line("task.lifecycle.proposed", {
        kind: "task_lifecycle",
        task_id: taskId,
        event: { kind: "proposed", task_id: taskId, task_kind: "tool.bash" },
      }),
    );
    fold.pushLine(
      line("task.lifecycle.tool_output_ref", {
        kind: "task_lifecycle",
        task_id: taskId,
        event: {
          kind: "tool_output_ref",
          task_id: taskId,
          output_ref: { id: `tool_patch-${taskId}-${callId}` },
        },
      }),
    );
    const result = fold.pushLine(
      line("tool.result", {
        kind: "tool_result",
        call_id: callId,
        text: "done",
        correlation_facts: { tool_name: "bash", outcome: "success" },
      }),
    );
    expect(result[0]).toMatchObject({ type: "tool.updated", callId: taskId });
  });

  it("falls back to the call id for unmapped tool results", () => {
    const fold = new MuseExecFold();
    const result = fold.pushLine(
      line("tool.result", {
        kind: "tool_result",
        call_id: "call_orphan",
        text: "done",
        correlation_facts: { tool_name: "bash", outcome: "success" },
      }),
    );
    expect(result).toEqual([
      {
        type: "tool.updated",
        callId: "call_orphan",
        title: "bash",
        status: "completed",
        detail: "done",
        paths: undefined,
      },
    ]);
  });

  it("tracks interleaved tool tasks independently", () => {
    const fold = new MuseExecFold();
    for (const taskId of ["task-a", "task-b"]) {
      fold.pushLine(
        line("task.lifecycle.proposed", {
          kind: "task_lifecycle",
          task_id: taskId,
          event: { kind: "proposed", task_id: taskId, task_kind: "tool.read" },
        }),
      );
    }
    const updateB = fold.pushLine(
      line("task.lifecycle.output", {
        kind: "task_lifecycle",
        task_id: "task-b",
        event: { kind: "output", task_id: "task-b", chunk: "b-out", final_result: false },
      }),
    );
    expect(updateB).toEqual([{ type: "tool.updated", callId: "task-b", detail: "b-out" }]);
  });
});
