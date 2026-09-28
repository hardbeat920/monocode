import { afterEach, describe, expect, it, vi } from "vitest";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { SendTurnInput } from "../src/integrations/harness/core/types";
import type { HostProvider } from "./providers";
import { HostEngine, parseCommand } from "./engine";
import { HostStore } from "./store";
import { writeAttachmentChunk } from "./attachments";

const cleanups: Array<() => Promise<void> | void> = [];
afterEach(async () => {
  for (const cleanup of cleanups.splice(0).reverse()) await cleanup();
});

function setup(harness: "codex" | "claude" = "codex") {
  const directory = mkdtempSync(join(tmpdir(), "monocode-engine-test-"));
  const store = new HostStore(join(directory, "host.db"));
  const project = store.addProject(directory, "Test");
  const turns: Array<{ input: SendTurnInput; finish: () => void }> = [];
  const provider: HostProvider = {
    send: vi.fn(
      (input) =>
        new Promise<void>((resolve) => {
          turns.push({ input, finish: resolve });
        }),
    ),
    cancel: vi.fn(async () => {
      turns.at(-1)?.finish();
    }),
    stop: vi.fn(async () => {
      turns.at(-1)?.finish();
    }),
    bind: vi.fn(),
    approve: vi.fn(),
    answer: vi.fn(),
  };
  const engine = new HostEngine(store, { codex: provider, claude: provider });
  const created = engine.command({
    type: "create",
    commandId: "create",
    projectId: project.id,
    harness,
    model: `${harness}:test`,
    runtimeMode: "supervised",
  });
  cleanups.push(async () => {
    await engine.close();
    store.close();
    rmSync(directory, { recursive: true, force: true });
  });
  return {
    directory,
    store,
    provider,
    project,
    engine,
    turns,
    id: created.sessionId,
  };
}

describe("headless session ownership", () => {
  it("stores a remote draft with an uploaded file, then sends it in plan mode", async () => {
    const { engine, store, turns, id } = setup();
    const fileId = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
    expect(
      writeAttachmentChunk(store, {
        id: fileId,
        offset: 0,
        size: 5,
        data: Buffer.from("hello").toString("base64"),
      }),
    ).toEqual({ offset: 5 });
    const attachment = {
      id: fileId,
      name: "notes.txt",
      mimeType: "text/plain",
      kind: "file" as const,
      size: 5,
    };
    engine.command({
      type: "draft",
      commandId: "draft-1",
      sessionId: id,
      text: "Plan this",
      attachments: [attachment],
    });
    expect(store.session(id).session.blocks[0]).toMatchObject({
      draft: true,
      attachments: [{ name: "notes.txt" }],
    });
    expect(store.summaries(store.session(id).projectId)[0].draft).toBe(true);
    engine.command({
      type: "send",
      commandId: "send-draft",
      sessionId: id,
      text: "Plan this",
      intent: "plan",
      draftBlockId: "draft-1",
    });
    await vi.waitFor(() => expect(turns).toHaveLength(1));
    expect(turns[0].input).toMatchObject({
      intent: "plan",
      attachments: [{ name: "notes.txt", size: 5 }],
    });
    expect(turns[0].input.attachments?.[0].path).toContain(fileId);
    expect(store.summaries(store.session(id).projectId)[0].draft).toBe(false);
    turns[0].finish();
  });

  it("passes an uploaded image to the host provider on an attachment-only turn", async () => {
    const { engine, store, turns, id } = setup("claude");
    const fileId = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
    const image = Buffer.from("image-bytes");
    writeAttachmentChunk(store, {
      id: fileId,
      offset: 0,
      size: image.length,
      data: image.toString("base64"),
    });
    engine.command({
      type: "send",
      commandId: "image-turn",
      sessionId: id,
      text: "",
      attachments: [
        {
          id: fileId,
          name: "shot.png",
          mimeType: "image/png",
          kind: "image",
          size: image.length,
        },
      ],
    });
    await vi.waitFor(() => expect(turns).toHaveLength(1));
    expect(turns[0].input.attachments?.[0]).toMatchObject({
      name: "shot.png",
      data: image.toString("base64"),
    });
    turns[0].finish();
  });

  it("removes a remote draft without starting the provider", () => {
    const { engine, store, provider, id } = setup();
    engine.command({
      type: "draft",
      commandId: "draft-2",
      sessionId: id,
      text: "Later",
    });
    engine.command({
      type: "removeDraft",
      commandId: "remove-2",
      sessionId: id,
      draftBlockId: "draft-2",
    });
    expect(store.session(id).session.blocks).toEqual([]);
    expect(provider.send).not.toHaveBeenCalled();
  });

  it("marks a reviewed host plan as built after its build turn", async () => {
    const { engine, store, turns, id } = setup();
    engine.command({
      type: "send",
      commandId: "plan-turn",
      sessionId: id,
      text: "Plan this",
      intent: "plan",
    });
    await vi.waitFor(() => expect(turns).toHaveLength(1));
    turns[0].input.onEvent({ type: "plan", text: "# Steps\n\n1. Change code" });
    turns[0].finish();
    await vi.waitFor(() => expect(store.session(id).status).toBe("idle"));
    const plan = store
      .session(id)
      .session.blocks.find((block) => block.role === "plan")!;
    engine.command({
      type: "send",
      commandId: "build-turn",
      sessionId: id,
      text: `Build the approved plan:\n\n${plan.text}`,
      intent: "build",
      planBlockId: plan.id,
    });
    expect(
      store.session(id).session.blocks.find((block) => block.id === plan.id)
        ?.plan?.status,
    ).toBe("building");
    await vi.waitFor(() => expect(turns).toHaveLength(2));
    turns[1].finish();
    await vi.waitFor(() => expect(store.session(id).status).toBe("idle"));
    expect(
      store.session(id).session.blocks.find((block) => block.id === plan.id)
        ?.plan?.status,
    ).toBe("built");
  });

  it("keeps a manually renamed title when first-turn generation finishes later", async () => {
    const { engine, store, provider, turns, id } = setup();
    let finishTitle: (title: {
      title: string;
      workItem: null;
    }) => void = () => {};
    provider.generateTitle = vi.fn(
      () =>
        new Promise((resolve) => {
          finishTitle = resolve;
        }),
    );
    engine.command({
      type: "send",
      commandId: "name-first-turn",
      sessionId: id,
      text: "Fix remote project titles",
    });
    await vi.waitFor(() => expect(turns).toHaveLength(1));
    engine.updateSession(id, { title: "codex · My own title" });
    finishTitle({ title: "Generated title", workItem: null });
    await vi.waitFor(() =>
      expect(provider.generateTitle).toHaveBeenCalledTimes(1),
    );
    turns[0].finish();
    await vi.waitFor(() => expect(store.session(id).status).toBe("idle"));
    expect(store.session(id).session.title).toBe("codex · My own title");
  });

  it("keeps remote card changes in host history and removes deleted sessions", () => {
    const { store, project, id } = setup();
    const initial = store.summaries(project.id)[0];
    expect(initial.model).toBe("codex:test");

    const updated = store.updateSession(id, {
      title: "Codex · Renamed",
      pinned: true,
      archived: true,
      linkedWorkItem: {
        kind: "issue",
        repo: "example/repo",
        number: 42,
        url: "https://github.com/example/repo/issues/42",
      },
    });
    expect(updated).toMatchObject({
      title: "Codex · Renamed",
      pinned: true,
      archived: true,
      model: "codex:test",
      linkedWorkItem: { number: 42 },
    });
    expect(store.summaries(project.id)[0]).toMatchObject({
      title: updated.title,
      pinned: true,
      archived: true,
      revision: updated.revision,
    });
    expect(store.sync(id, initial.revision)).toMatchObject({ kind: "delta" });
    store.updateSession(id, { linkedWorkItem: null });
    expect(store.summaries(project.id)[0].linkedWorkItem).toBeUndefined();

    store.deleteSession(id);
    expect(store.summaries(project.id)).toEqual([]);
    expect(() => store.session(id)).toThrow("Session not found");
  });

  it("preserves the transaction error and invalidates cached state if rollback fails", () => {
    const { store, id } = setup();
    const cached = store.session(id);
    store.db.prepare("UPDATE sessions SET snapshot=? WHERE id=?").run(
      JSON.stringify({
        ...cached,
        session: { ...cached.session, title: "Updated" },
      }),
      id,
    );
    const exec = store.db.exec.bind(store.db);
    const rollback = vi.spyOn(store.db, "exec").mockImplementation((sql) => {
      if (sql === "ROLLBACK") throw new Error("rollback failed");
      return exec(sql);
    });
    const log = vi.spyOn(console, "error").mockImplementation(() => {});
    const original = new Error("transaction failed");
    try {
      expect(() =>
        store.transaction(() => {
          throw original;
        }),
      ).toThrow(original);
      expect(store.session(id).session.title).toBe("Updated");
    } finally {
      rollback.mockRestore();
      log.mockRestore();
      store.db.exec("ROLLBACK");
    }
  });
  it("keeps the checkout idle while a branch switch is in progress", async () => {
    const { engine, project, id, turns } = setup();
    let finishSwitch = () => {};
    const switching = engine.withIdleProject(
      project.id,
      () =>
        new Promise<void>((resolve) => {
          finishSwitch = resolve;
        }),
    );
    expect(() =>
      engine.command({
        type: "send",
        commandId: "during-switch",
        sessionId: id,
        text: "Work",
      }),
    ).toThrow("branch switch");
    expect(() =>
      engine.command({
        type: "create",
        commandId: "new-during-switch",
        projectId: project.id,
        harness: "codex",
        model: "codex:test",
        runtimeMode: "supervised",
      }),
    ).toThrow("branch switch");
    finishSwitch();
    await switching;
    engine.command({
      type: "send",
      commandId: "after-switch",
      sessionId: id,
      text: "Work",
    });
    await vi.waitFor(() => expect(turns).toHaveLength(1));
    turns[0].finish();
  });

  it("runs provider context compaction once and persists its transcript marker", async () => {
    const { engine, store, provider, id } = setup();
    let finishCompact = () => {};
    provider.compact = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          finishCompact = resolve;
        }),
    );
    const command = { type: "compact", commandId: "compact", sessionId: id };
    const receipt = engine.command(command);
    expect(engine.command(command)).toEqual(receipt);
    await vi.waitFor(() => expect(provider.compact).toHaveBeenCalledTimes(1));
    expect(store.session(id).session.blocks).toContainEqual(
      expect.objectContaining({ text: "/compact" }),
    );
    finishCompact();
    await vi.waitFor(() => expect(store.session(id).status).toBe("idle"));
  });

  it("persists model and permission changes for the next turn and rejects changes mid-turn", async () => {
    const { engine, store, turns, id } = setup();
    const change = {
      type: "configure",
      commandId: "settings",
      sessionId: id,
      model: "codex:new",
      modelSettings: { reasoningEffort: "high" },
      runtimeMode: "full-access",
    };
    const receipt = engine.command(change);
    expect(engine.command(change)).toEqual(receipt);
    expect(store.session(id).session).toMatchObject({
      model: "codex:new",
      modelSettings: { reasoningEffort: "high" },
      runtimeMode: "full-access",
    });
    engine.command({
      type: "send",
      commandId: "turn",
      sessionId: id,
      text: "Continue",
    });
    await vi.waitFor(() => expect(turns).toHaveLength(1));
    expect(turns[0].input).toMatchObject({
      model: "codex:new",
      modelSettings: { reasoningEffort: "high" },
      runtimeMode: "full-access",
    });
    expect(() => engine.command({ ...change, commandId: "later" })).toThrow(
      "current turn",
    );
    turns[0].finish();
  });
  it("keeps working with no client, persists output, and deduplicates a lost acknowledgement", async () => {
    const { engine, store, turns, provider, id } = setup();
    const command = {
      type: "send",
      commandId: "send-once",
      sessionId: id,
      text: "Do the work",
    };
    const receipt = engine.command(command);
    expect(engine.command(command)).toEqual(receipt);
    await vi.waitFor(() => expect(turns).toHaveLength(1));
    expect(provider.send).toHaveBeenCalledTimes(1);
    const before = store.session(id).revision;
    turns[0].input.onEvent({
      type: "session.providerBound",
      providerSessionId: "provider-thread",
    });
    turns[0].input.onEvent({
      type: "message.delta",
      text: "still working while disconnected",
    });
    turns[0].finish();
    await vi.waitFor(() => expect(store.session(id).status).toBe("idle"));
    expect(
      store
        .session(id)
        .session.blocks.some((block) => block.text.includes("still working")),
    ).toBe(true);
    expect(store.events(id, before).events?.length).toBeGreaterThan(1);
    expect(engine.command(command)).toEqual(receipt);
    expect(provider.send).toHaveBeenCalledTimes(1);
    expect(provider.bind).toHaveBeenCalledWith(
      id,
      "provider-thread",
      expect.any(String),
    );
    expect(() =>
      engine.command({ ...command, text: "Changed payload" }),
    ).toThrow("different payload");
  });

  it.each(["codex", "claude"] as const)(
    "keeps %s turn timing and model provenance after settlement and reconnect",
    async (harness) => {
      const { engine, store, turns, id } = setup(harness);
      engine.command({
        type: "send",
        commandId: "first-turn",
        sessionId: id,
        text: "Inspect the project",
      });
      await vi.waitFor(() => expect(turns).toHaveLength(1));
      const running = store.session(id);
      expect(running.session.blocks[0]).toMatchObject({
        id: "first-turn",
        startedAt: expect.any(Number),
        turnModel: { harness, id: `${harness}:test` },
      });
      turns[0].input.onEvent({ type: "message.delta", text: "Found it" });
      turns[0].finish();
      await vi.waitFor(() => expect(store.session(id).status).toBe("idle"));

      const reconnected = store.sync(id);
      expect(reconnected.kind).toBe("snapshot");
      if (reconnected.kind !== "snapshot") return;
      expect(reconnected.value.session.blocks[0]).toMatchObject({
        id: "first-turn",
        startedAt: expect.any(Number),
        durationMs: expect.any(Number),
        turnModel: { harness, id: `${harness}:test` },
      });
      expect(
        reconnected.value.session.blocks[0].durationMs,
      ).toBeGreaterThanOrEqual(0);
      expect(reconnected.value.session.blocks[1].text).toBe("Found it");

      const delta = store.sync(id, running.revision);
      expect(delta.kind).toBe("delta");
      if (delta.kind === "delta")
        expect(
          delta.blocks.some(
            (block) => block.id === "first-turn" && block.durationMs != null,
          ),
        ).toBe(true);
    },
  );

  it("serializes concurrent sends and accepts only one approval decision for a run", async () => {
    const { engine, store, turns, provider, id } = setup();
    engine.command({
      type: "send",
      commandId: "send",
      sessionId: id,
      text: "Work",
    });
    expect(() =>
      engine.command({
        type: "send",
        commandId: "other-send",
        sessionId: id,
        text: "More work",
      }),
    ).toThrow("already running");
    await vi.waitFor(() => expect(turns).toHaveLength(1));
    turns[0].input.onEvent({
      type: "approval.requested",
      requestId: 7,
      title: "Run a command?",
    });
    const runId = store.session(id).runId!;
    const approval = {
      type: "approve",
      commandId: "approval-1",
      sessionId: id,
      runId,
      requestId: 7,
      decision: "allow",
    };
    expect(() => engine.command({ ...approval, runId: "stale" })).toThrow(
      "finished or replaced",
    );
    engine.command(approval);
    engine.command(approval);
    expect(() =>
      engine.command({
        ...approval,
        commandId: "approval-2",
        decision: "deny",
      }),
    ).toThrow("already resolved");
    expect(provider.approve).toHaveBeenCalledTimes(1);
    expect(provider.approve).toHaveBeenCalledWith(id, 7, "allow");
  });

  it("stores pending questions and rejects a second device's stale answer", async () => {
    const { engine, store, turns, provider, id } = setup();
    engine.command({
      type: "send",
      commandId: "send",
      sessionId: id,
      text: "Work",
    });
    await vi.waitFor(() => expect(turns).toHaveLength(1));
    turns[0].input.onEvent({
      type: "question.asked",
      requestId: 3,
      questions: [
        {
          id: "q1",
          prompt: "Choose",
          multiSelect: false,
          allowCustom: false,
          options: [{ id: "yes", label: "Yes" }],
        },
      ],
    });
    const reply = {
      type: "answer",
      commandId: "answer",
      sessionId: id,
      runId: store.session(id).runId,
      requestId: 3,
      reply: { kind: "answered", answers: { q1: ["yes"] } },
    };
    engine.command(reply);
    expect(store.session(id).session.pendingQuestion).toBeUndefined();
    expect(() =>
      engine.command({ ...reply, commandId: "other-answer" }),
    ).toThrow("already resolved");
    expect(provider.answer).toHaveBeenCalledTimes(1);
  });

  it("recovers interrupted durable state without replaying an uncertain provider send", async () => {
    const { store, provider, id } = setup();
    const value = store.session(id);
    store.transaction(() =>
      store.save(
        {
          ...value,
          revision: value.revision + 1,
          status: "running",
          runId: "old-run",
          session: {
            ...value.session,
            busy: true,
            providerSessionId: "retained",
            blocks: [
              {
                id: "interrupted-turn",
                role: "user",
                text: "Work",
                startedAt: value.updatedAt - 2_000,
              },
            ],
          },
        },
        { type: "accepted" },
      ),
    );
    const recovered = new HostEngine(store, { codex: provider });
    expect(store.session(id).status).toBe("interrupted");
    expect(store.session(id).session.busy).toBe(false);
    expect(store.session(id).session.blocks[0].durationMs).toBe(2_000);
    expect(provider.send).not.toHaveBeenCalled();
    expect(provider.bind).toHaveBeenCalledWith(
      id,
      "retained",
      value.session.cwd,
    );
    await recovered.close();
  });

  it("retries a failed event write and settles the stopped turn", async () => {
    const { engine, store, turns, id } = setup();
    engine.command({
      type: "send",
      commandId: "send",
      sessionId: id,
      text: "Work",
    });
    await vi.waitFor(() => expect(turns).toHaveLength(1));
    const original = store.save.bind(store);
    let failed = false;
    vi.spyOn(store, "save").mockImplementation((value, event) => {
      if (!failed && (event as { type?: string }).type === "events") {
        failed = true;
        throw new Error("temporary storage error");
      }
      return original(value, event);
    });
    const log = vi.spyOn(console, "error").mockImplementation(() => {});
    try {
      turns[0].input.onEvent({
        type: "message.delta",
        text: "Retained output",
      });
      turns[0].finish();
      await vi.waitFor(
        () => expect(store.session(id).status).toBe("interrupted"),
        {
          timeout: 4_000,
        },
      );
      expect(
        store
          .session(id)
          .session.blocks.some((block) => block.text === "Retained output"),
      ).toBe(true);
      expect(store.session(id).session.busy).toBe(false);
    } finally {
      log.mockRestore();
    }
  });

  it("retries a failed final settlement", async () => {
    const { engine, store, turns, id } = setup();
    engine.command({
      type: "send",
      commandId: "send",
      sessionId: id,
      text: "Work",
    });
    await vi.waitFor(() => expect(turns).toHaveLength(1));
    const original = store.save.bind(store);
    let failed = false;
    vi.spyOn(store, "save").mockImplementation((value, event) => {
      if (!failed && (event as { type?: string }).type === "settled") {
        failed = true;
        throw new Error("temporary storage error");
      }
      return original(value, event);
    });
    const log = vi.spyOn(console, "error").mockImplementation(() => {});
    try {
      turns[0].finish();
      await vi.waitFor(
        () => expect(store.session(id).status).toBe("interrupted"),
        {
          timeout: 4_000,
        },
      );
      expect(store.session(id).session.busy).toBe(false);
    } finally {
      log.mockRestore();
    }
  });

  it("batches streamed output and syncs only changed blocks", async () => {
    const { engine, store, turns, id, project } = setup();
    engine.command({
      type: "send",
      commandId: "send",
      sessionId: id,
      text: "Work",
    });
    await vi.waitFor(() => expect(turns).toHaveLength(1));
    const started = store.session(id).revision;
    for (let index = 0; index < 50; index++)
      turns[0].input.onEvent({
        type: "message.delta",
        text: `chunk ${index} `,
      });
    expect(store.session(id).revision).toBe(started);
    await vi.waitFor(() =>
      expect(store.session(id).revision).toBe(started + 1),
    );
    const sync = store.sync(id, started);
    expect(sync.kind).toBe("delta");
    if (sync.kind !== "delta") return;
    expect(sync.blocks.map((block) => block.role)).toEqual(["assistant"]);
    expect(sync.blockIds).toHaveLength(2);

    turns[0].input.onEvent({
      type: "approval.requested",
      requestId: 1,
      title: "Run a command?",
    });
    expect(store.session(id).revision).toBe(started + 2);

    const streamed = store.session(id).revision;
    turns[0].finish();
    await vi.waitFor(() => expect(store.session(id).status).toBe("idle"));
    const settled = store.sync(id, streamed);
    if (settled.kind !== "delta") throw new Error("Expected a delta");
    expect(
      settled.blocks.some(
        (block) => block.role === "user" && block.durationMs != null,
      ),
    ).toBe(true);
    expect(store.sync(id, store.session(id).revision).kind).toBe("unchanged");
    expect(store.sync(id).kind).toBe("snapshot");
    expect(store.summaries(project.id)[0]).toMatchObject({
      id,
      status: "idle",
      title: "codex · Work",
    });
  });

  it("requires snapshot recovery when the client's event cursor is invalid", () => {
    const { store, id } = setup();
    expect(store.events(id, 100_000).snapshot?.session.id).toBe(id);
  });

  it("validates untrusted commands before execution", () => {
    expect(() =>
      parseCommand({ type: "send", commandId: "x", sessionId: "y", text: "" }),
    ).toThrow();
    expect(() =>
      parseCommand({
        type: "create",
        commandId: "x",
        projectId: "y",
        harness: "shell",
        model: "x",
        runtimeMode: "auto",
      }),
    ).toThrow();
    expect(() =>
      parseCommand({
        type: "answer",
        commandId: "x",
        sessionId: "y",
        runId: "z",
        requestId: 1,
        reply: { kind: "answered", answers: { a: [42] } },
      }),
    ).toThrow();
  });
});
