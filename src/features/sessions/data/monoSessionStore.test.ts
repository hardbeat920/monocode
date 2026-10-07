import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  getSession,
  getMonoTranscriptPage,
  upsertSession,
  persistFingerprint,
} from "./sessionStore";
import type { Session } from "../model/session";
import {
  monoSessionCompletionMessage,
  monoSessionCompletionResult,
  MonoSessionCompletionBatches,
} from "../../monos/model/monoSessionCompletion";
import { canSteerQueuedHead } from "../model/messageQueue";
import { recordArtifactCard } from "../../artifacts/artifacts";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../../monos/model/mono", () => ({
  isMonoSession: (id: string) => id.startsWith("mono"),
}));
const call = vi.mocked(invoke);
let id = 0;

function record(mono = true) {
  return {
    id: `${mono ? "mono" : "normal"}-${++id}`,
    cwd: "/tmp",
    harness: "codex",
    model: "model",
    modelSettings: {},
    runtimeMode: "default",
    title: "Chat",
    createdAt: 1,
    updatedAt: 2,
    blocks: [
      { id: "u", role: "user", text: "Recent question" },
      { id: "a", role: "assistant", text: "Recent answer" },
    ],
    ...(mono ? { monoTranscript: { before: 1000, firstBlockId: "u" } } : {}),
  };
}

beforeEach(() => call.mockReset());
async function load(mono = true): Promise<Session> {
  const stored = record(mono);
  call.mockResolvedValueOnce(stored);
  return (await getSession(stored.id))!;
}
function saved(session: Session) {
  return { ...session, createdAt: 1, updatedAt: 2 };
}

describe("Mono database pagination and writes", () => {
  it("keeps document references on the source turn after saving and reopening", async () => {
    let session = await load();
    const card = {
      id: "artifact-report",
      kind: "document" as const,
      title: "PR review",
      summary: "Merge queue",
    };
    const before = persistFingerprint(session);
    session = recordArtifactCard(session, session.blocks[0].id, card);
    expect(persistFingerprint(session)).not.toBe(before);
    call.mockResolvedValueOnce(saved(session));
    await upsertSession(session);
    const payload = (call.mock.calls.at(-1)![1] as { session: Session }).session;
    expect(payload.blocks[0].artifactCards).toEqual([card]);
    call.mockResolvedValueOnce(saved(payload));
    const restored = (await getSession(session.id))!;
    expect(restored.blocks[0].artifactCards).toEqual([card]);
  });
  it("preserves all results and the session count in a combined report across queue and transcript saves", async () => {
    const session = await load();
    const batches = new MonoSessionCompletionBatches((_, message) => {
      session.queuedMessages = [message];
    });
    for (const id of ["first", "second"]) {
      batches.watch(
        { monoId: session.id, turn: 1 },
        id,
      )(
        monoSessionCompletionResult({
          requestId: id,
          sessionId: id,
          project: "/tmp",
          prompt: `Check ${id}`,
          outcome: { status: "completed", text: `Finished ${id}` },
        }),
      );
    }
    batches.closeInactive(() => false);
    const notification = session.queuedMessages![0];
    call.mockResolvedValueOnce(saved(session));
    await upsertSession(session);
    const payload = (call.mock.calls.at(-1)![1] as { session: Session })
      .session;
    expect(payload.queuedMessages?.[0]).toEqual(notification);
    call.mockResolvedValueOnce({ ...payload, createdAt: 1, updatedAt: 2 });
    const restored = (await getSession(session.id))!;
    expect(
      restored.queuedMessages?.[0].monoSessionCompletion?.sessionCount,
    ).toBe(2);
    expect(restored.queuedMessages?.[0].text).toContain("Finished first");
    expect(restored.queuedMessages?.[0].text).toContain("Finished second");
    restored.blocks.push({
      id: "delivery",
      role: "user",
      text: notification.text,
      internal: true,
      appRequestId: notification.id,
      monoSessionCompletion: notification.monoSessionCompletion,
    });
    call.mockResolvedValueOnce(saved(restored));
    await upsertSession(restored);
    const delivered = (
      call.mock.calls.at(-1)![1] as { session: Session }
    ).session.blocks.at(-1)!;
    expect(delivered.monoSessionCompletion?.sessionCount).toBe(2);
  });

  it("keeps completion notifications idle-only across persistence and reload", async () => {
    const session = await load();
    const notification = monoSessionCompletionMessage({
      sessionId: "worker",
      requestId: "app-mono-monitor",
      project: "/tmp",
      prompt: "Review",
      outcome: { status: "completed", text: "Review done" },
    });
    session.queuedMessages = [notification];
    session.queueStatus = "active";
    call.mockResolvedValueOnce(saved(session));
    await upsertSession(session);
    const payload = (
      call.mock.calls.at(-1)![1] as { session: Record<string, unknown> }
    ).session;
    expect(payload.queuedMessages).toEqual([notification]);
    call.mockResolvedValueOnce({
      ...record(),
      ...payload,
      createdAt: 1,
      updatedAt: 2,
    });
    const restored = (await getSession(session.id))!;
    expect(restored.queuedMessages?.[0]).toEqual(notification);
    expect(
      canSteerQueuedHead({
        ...restored,
        busy: true,
        turnReady: true,
        queueStatus: "active",
      }),
    ).toBe(false);
    restored.blocks = [
      ...restored.blocks,
      {
        id: "delivered",
        role: "user",
        text: notification.text,
        internal: true,
        appRequestId: notification.id,
        monoSessionCompletion: notification.monoSessionCompletion,
      },
    ];
    call.mockResolvedValueOnce(saved(restored));
    await upsertSession(restored);
    const delivered = (
      call.mock.calls.at(-1)![1] as { session: Session }
    ).session.blocks.at(-1)!;
    expect(delivered).toMatchObject({
      internal: true,
      appRequestId: notification.id,
      monoSessionCompletion: notification.monoSessionCompletion,
    });
  });

  it("persists queued messages and attachment bytes, then recovers them paused in order", async () => {
    const session = await load();
    const original = persistFingerprint(session);
    const noteCard = {
      id: "note",
      slug: "todo",
      title: "Todo",
      body: "Do this",
    };
    session.queuedMessages = [
      {
        id: "waiting-first",
        blockId: "optimistic-first",
        text: "First",
        attachments: [],
        noteCard,
      },
      {
        id: "waiting-second",
        text: "",
        attachments: [
          {
            id: "image",
            name: "Paste.png",
            mimeType: "image/png",
            kind: "image",
            size: 12,
            data: "image-bytes",
            previewUrl: "blob:temporary",
          },
        ],
        error: "Connection lost",
      },
    ];
    session.queueStatus = "active";
    expect(persistFingerprint(session)).not.toBe(original);
    call.mockResolvedValueOnce(saved(session));
    await upsertSession(session);
    const payload = (
      call.mock.calls.at(-1)![1] as { session: Record<string, unknown> }
    ).session;
    expect(payload.queuedMessages).toEqual([
      session.queuedMessages[0],
      {
        ...session.queuedMessages[1],
        attachments: [expect.objectContaining({ data: "image-bytes" })],
      },
    ]);
    expect(
      (payload.queuedMessages as typeof session.queuedMessages)[1]
        .attachments[0].previewUrl,
    ).toBeUndefined();
    call.mockResolvedValueOnce({
      ...record(),
      ...payload,
      createdAt: 1,
      updatedAt: 2,
    });
    const restored = (await getSession(session.id))!;
    expect(restored.queueStatus).toBe("paused");
    expect(restored.busy).toBe(false);
    expect(restored.queuedMessages?.map((message) => message.id)).toEqual([
      "waiting-first",
      "waiting-second",
    ]);
    expect(restored.queuedMessages?.[0].noteCard).toEqual(noteCard);
    expect(restored.queuedMessages?.[0].blockId).toBe("optimistic-first");
    expect(restored.queuedMessages?.[1].attachments[0].data).toBe(
      "image-bytes",
    );
    expect(restored.queuedMessages?.[1].error).toBe("Connection lost");
    restored.queuedMessages = [];
    call.mockResolvedValueOnce(saved(restored));
    await upsertSession(restored);
    expect(call).toHaveBeenLastCalledWith(
      "mono_session_upsert",
      expect.objectContaining({
        session: expect.not.objectContaining({
          queuedMessages: expect.anything(),
        }),
      }),
    );
  });
  it("keeps habit identity and posting time through saves, reloads and older pages", async () => {
    const monoHabit = { id: "habit", name: "Morning check", at: 1_000 };
    const report = {
      id: "report",
      role: "assistant" as const,
      text: "CI failed.",
      monoHabit,
    };
    const stored = { ...record(), blocks: [report] };
    call.mockResolvedValueOnce(stored);
    const session = (await getSession(stored.id))!;
    expect(session.blocks[0].monoHabit).toEqual(monoHabit);

    session.blocks = [...session.blocks, { ...report, id: "next-report" }];
    call.mockResolvedValueOnce(saved(session));
    await upsertSession(session);
    expect(call).toHaveBeenLastCalledWith(
      "mono_session_upsert",
      expect.objectContaining({
        session: expect.objectContaining({
          blocks: [expect.objectContaining({ id: "next-report", monoHabit })],
        }),
      }),
    );

    call.mockResolvedValueOnce({
      blocks: [report],
      before: null,
      hasNewer: true,
    });
    const page = await getMonoTranscriptPage(session.id, { before: 1_000 });
    expect(page.blocks[0].monoHabit).toEqual(monoHabit);
  });

  it("preserves a Mono after removing its only loaded draft and appends to the saved tail", async () => {
    const session = await load();
    session.blocks = [];
    call.mockResolvedValueOnce(saved(session));
    await upsertSession(session);
    expect(call).toHaveBeenLastCalledWith(
      "mono_session_upsert",
      expect.objectContaining({
        fromBlockId: "u",
        appendOnly: false,
        session: expect.objectContaining({ blocks: [] }),
      }),
    );
    session.blocks = [{ id: "new", role: "user", text: "Fresh message" }];
    call.mockResolvedValueOnce(saved(session));
    await upsertSession(session);
    expect(call).toHaveBeenLastCalledWith(
      "mono_session_upsert",
      expect.objectContaining({ fromBlockId: null, appendOnly: true }),
    );
  });

  it("uses the Mono command and requests older pages by cursor", async () => {
    const session = await load();
    expect(call).toHaveBeenCalledWith("mono_session_get", {
      sessionId: session.id,
    });
    expect(session.monoTranscript?.before).toBe(1000);
    call.mockResolvedValueOnce({ blocks: [], before: 980, hasNewer: true });
    await getMonoTranscriptPage(session.id, { before: 1000 });
    expect(call).toHaveBeenLastCalledWith("mono_session_page", {
      sessionId: session.id,
      before: 1000,
      beforeBlockId: null,
      aroundBlockId: null,
    });
  });

  it("writes only appended or edited blocks and anchors the partial window", async () => {
    const session = await load();
    session.blocks = [
      ...session.blocks,
      { id: "next", role: "user", text: "Next" },
    ];
    call.mockResolvedValueOnce(saved(session));
    await upsertSession(session);
    expect(call).toHaveBeenLastCalledWith(
      "mono_session_upsert",
      expect.objectContaining({
        afterBlockId: "a",
        fromBlockId: null,
        blocksChanged: true,
        session: expect.objectContaining({
          blocks: [expect.objectContaining({ id: "next" })],
        }),
      }),
    );
    session.blocks = [
      { ...session.blocks[0], text: "Edited" },
      ...session.blocks.slice(1),
    ];
    call.mockResolvedValueOnce(saved(session));
    await upsertSession(session);
    expect(call).toHaveBeenLastCalledWith(
      "mono_session_upsert",
      expect.objectContaining({
        afterBlockId: null,
        fromBlockId: "u",
        blocksChanged: true,
      }),
    );
  });

  it("serializes overlapping saves against the last successful suffix", async () => {
    const session = await load();
    const first = {
      ...session,
      blocks: [
        ...session.blocks,
        { id: "one", role: "user" as const, text: "One" },
      ],
    };
    const second = {
      ...first,
      blocks: [
        ...first.blocks,
        { id: "two", role: "assistant" as const, text: "Two" },
      ],
    };
    call.mockResolvedValue(saved(second));
    await Promise.all([upsertSession(first), upsertSession(second)]);
    expect(
      call.mock.calls
        .slice(1)
        .map(([, args]) => (args as { afterBlockId: string }).afterBlockId),
    ).toEqual(["a", "one"]);
    expect(call).toHaveBeenLastCalledWith(
      "mono_session_upsert",
      expect.objectContaining({
        session: expect.objectContaining({
          blocks: [expect.objectContaining({ id: "two" })],
        }),
      }),
    );
  });

  it("retries failed writes and persists rewinds and metadata without losing history", async () => {
    const session = await load();
    session.blocks = session.blocks.slice(0, 1);
    call.mockRejectedValueOnce(new Error("Disk busy"));
    await expect(upsertSession(session)).rejects.toThrow("Disk busy");
    call.mockResolvedValueOnce(saved(session));
    await upsertSession(session);
    expect(call).toHaveBeenLastCalledWith(
      "mono_session_upsert",
      expect.objectContaining({
        afterBlockId: "u",
        blocksChanged: true,
        session: expect.objectContaining({ blocks: [] }),
      }),
    );
    session.title = "Renamed";
    call.mockResolvedValueOnce(saved(session));
    await upsertSession(session);
    expect(call).toHaveBeenLastCalledWith(
      "mono_session_upsert",
      expect.objectContaining({
        blocksChanged: false,
        session: expect.objectContaining({ title: "Renamed", blocks: [] }),
      }),
    );
  });

  it("keeps normal session loading and complete saves unchanged", async () => {
    const session = await load(false);
    expect(call).toHaveBeenCalledWith("session_get", { sessionId: session.id });
    call.mockResolvedValueOnce(saved(session));
    await upsertSession(session);
    expect(call).toHaveBeenLastCalledWith(
      "session_upsert",
      expect.objectContaining({
        session: expect.objectContaining({
          blocks: [
            expect.objectContaining({ id: "u" }),
            expect.objectContaining({ id: "a" }),
          ],
        }),
      }),
    );
  });
});
