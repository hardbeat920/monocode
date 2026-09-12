import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Attachment } from "../session";

const sent: Array<{ method: string; params: { input?: unknown[] } }> = [];
let onLine: ((line: string) => void) | undefined;
let keepTurnOpen = false;

vi.mock("./child", () => ({
  resolveCodexBinary: async () => ({ path: "/fake/codex" }),
  spawnChild: async () => undefined,
  killChild: async () => undefined,
  unwatchChild: () => undefined,
  watchChild: (_id: string, line: (l: string) => void) => {
    onLine = line;
  },
  writeChild: async (_id: string, line: string) => {
    const message = JSON.parse(line);
    sent.push(message);
    if (message.id == null) return;
    const result =
      message.method === "thread/start"
        ? { thread: { id: "thr_repro" } }
        : message.method === "turn/start"
          ? { turn: { id: "turn_repro", status: "inProgress" } }
          : {};
    onLine!(JSON.stringify({ id: message.id, result }));
    if (message.method === "turn/start") {
      onLine!(
        JSON.stringify({
          method: "turn/started",
          params: { turn: { id: "turn_repro" } },
        }),
      );
      if (!keepTurnOpen) completeTurn();
    }
  },
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: async (command: string, args: { paths?: string[] }) => {
    if (command === "inspect_paths") {
      return args.paths!.map((path) => ({
        path,
        name: path.split("/").at(-1),
        size: path.endsWith("large.png") ? 21 * 1024 * 1024 : 100,
        isDir: false,
      }));
    }
    if (command === "read_file_base64") return "aW1hZ2U=";
    throw new Error(`Unexpected command: ${command}`);
  },
}));

const { sendCodexTurn, steerCodexTurn, stopCodexSession, __codexTestReset } =
  await import("./codex");
const { attachmentsFromPaths, prepareAttachments, promptBlocks } =
  await import("../attachments");

function completeTurn() {
  onLine!(
    JSON.stringify({
      method: "turn/completed",
      params: { turn: { id: "turn_repro", status: "completed" } },
    }),
  );
}

function send(text: string, attachments: Attachment[]) {
  return sendCodexTurn({
    sessionId: "issue174",
    cwd: "/repo",
    model: "codex:gpt-5.4",
    runtimeMode: "supervised",
    text,
    attachments,
    onEvent: () => {},
  });
}

async function prepared(name: string) {
  return prepareAttachments(
    await attachmentsFromPaths([`/tmp/issue174/${name}`]),
  );
}

async function outbound(
  method: "turn/start" | "turn/steer",
  text: string,
  attachments: Attachment[],
) {
  if (method === "turn/start") {
    await send(text, attachments);
  } else {
    keepTurnOpen = true;
    const running = send("Initial request", []);
    await vi.waitFor(() =>
      expect(sent.some((m) => m.method === "turn/start")).toBe(true),
    );
    try {
      await steerCodexTurn({
        sessionId: "issue174",
        cwd: "/repo",
        model: "codex:gpt-5.4",
        text,
        attachments,
      });
    } finally {
      completeTurn();
      await running;
    }
  }
  return sent.find((m) => m.method === method)?.params.input;
}

describe("Codex attachment delivery", () => {
  beforeEach(() => {
    sent.length = 0;
    onLine = undefined;
    keepTurnOpen = false;
  });
  afterEach(async () => {
    await stopCodexSession("issue174");
    __codexTestReset();
  });

  for (const method of ["turn/start", "turn/steer"] as const) {
    it.each([
      "report.pdf",
      "small.md",
      "server.log",
      "large.png",
      "drawing.svg",
    ])(`${method} forwards %s`, async (name) => {
      const files = await prepared(name);
      expect(files).toHaveLength(1);
      expect(promptBlocks("Read attached", files)[1]).toMatchObject({
        type: "resource_link",
      });
      const input = await outbound(method, "Read attached", files);
      expect(JSON.stringify(input)).toContain(files[0].path!);
    });

    it(`${method} sends an attachment-only PDF message`, async () => {
      const files = await prepared("report.pdf");
      const input = await outbound(method, "", files);
      expect(input).toBeDefined();
      expect(JSON.stringify(input)).toContain(files[0].path!);
    });

    it(`${method} preserves a normal PNG (control)`, async () => {
      const input = await outbound(
        method,
        "Read attached",
        await prepared("screenshot.png"),
      );
      expect(input).toEqual([
        { type: "text", text: "Read attached" },
        { type: "image", url: "data:image/png;base64,aW1hZ2U=" },
      ]);
    });

    it(`${method} uses native localImage inputs for images without embedded bytes`, async () => {
      const input = await outbound(method, "", await prepared("large.png"));
      expect(input).toEqual([
        { type: "localImage", path: "/tmp/issue174/large.png" },
      ]);
    });

    it(`${method} preserves mixed image and document inputs`, async () => {
      const files = [
        ...(await prepared("screenshot.png")),
        ...(await prepared("report.pdf")),
      ];
      const input = await outbound(method, "Review both", files);
      expect(input).toEqual([
        { type: "text", text: "Review both" },
        { type: "image", url: "data:image/png;base64,aW1hZ2U=" },
        {
          type: "text",
          text: 'Attached file (read from disk): "/tmp/issue174/report.pdf"',
        },
      ]);
    });

    it(`${method} rejects an attachment with no source`, async () => {
      const files = (await prepared("report.pdf")).map((file) => ({
        ...file,
        path: undefined,
      }));
      await expect(outbound(method, "Review", files)).rejects.toThrow(
        /report\.pdf.*no local file path/,
      );
    });
  }
});
