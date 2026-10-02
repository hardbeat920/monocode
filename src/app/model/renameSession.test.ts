import { beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { applyHarnessEvent } from "../../integrations/harness/core/apply";
import {
  getSession,
  listSessionsByProject,
  upsertSession,
  type SessionRecord,
} from "../../features/sessions/data/sessionStore";
import {
  canReplaceSessionTitle,
  newSession,
  titleFromPrompt,
  type Session,
} from "../../features/sessions/model/session";
import { renameSession } from "./renameSession";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
let stored: Map<string, SessionRecord>;
beforeEach(() => {
  stored = new Map();
  vi.mocked(invoke)
    .mockReset()
    .mockImplementation(async (command, args: any) => {
      if (command === "session_upsert") {
        const record = structuredClone({
          ...args.session,
          createdAt: 1,
          updatedAt: 2,
        });
        stored.set(record.id, record);
        return record;
      }
      if (command === "session_get")
        return structuredClone(stored.get(args.sessionId) ?? null);
      if (command === "session_list_by_project")
        return [...stored.values()].filter(
          (session) => session.cwd === args.cwd,
        );
      throw new Error(`Unexpected command: ${command}`);
    });
});

function chat(): Session {
  return {
    ...newSession("codex", "/repo"),
    id: "chat",
    title: titleFromPrompt("Fix tracking", "codex"),
    blocks: [{ id: "user", role: "user", text: "Fix tracking" }],
    busy: true,
  };
}

it("patches a busy session without losing queued harness events and awaits storage", async () => {
  let sessions = [chat()];
  let render = sessions;
  let queued!: (sessions: Session[]) => Session[];
  let release!: () => void;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  const storage = vi.mocked(invoke).getMockImplementation()!;
  vi.mocked(invoke).mockImplementationOnce(async (...args) => {
    await gate;
    return storage(...args);
  });
  let saved = false;
  const renaming = renameSession(
    "chat",
    "Fix tracking",
    () => sessions,
    (update) => {
      sessions = update(sessions);
      queued = update;
    },
  ).then(() => {
    saved = true;
  });
  // Immediate list readers use the live ref before React processes its queue.
  expect(sessions[0].titleIsExplicit).toBe(true);
  expect(canReplaceSessionTitle(sessions[0], sessions[0].title)).toBe(false);
  render = render.map((session) =>
    applyHarnessEvent(session, {
      type: "message.delta",
      text: "Still working",
    }),
  );
  render = queued(render);
  sessions = render;
  const savingProgress = upsertSession(sessions[0]);
  await Promise.resolve();
  expect(saved).toBe(false);
  release();
  await Promise.all([renaming, savingProgress]);
  expect(sessions[0].busy).toBe(true);
  expect(sessions[0].blocks.at(-1)?.text).toBe("Still working");
  const restored = await getSession("chat");
  expect(restored).toMatchObject({
    title: sessions[0].title,
    titleIsExplicit: true,
  });
  expect(restored?.blocks.at(-1)?.text).toBe("Still working");
  expect((await listSessionsByProject("/repo"))[0].title).toBe(
    sessions[0].title,
  );
});

it("renames a closed draft through storage and preserves an explicit prompt seed on reload", async () => {
  const session = chat();
  session.blocks[0].draft = true;
  await upsertSession(session);
  const update = vi.fn();
  await renameSession(session.id, "Fix tracking", () => [], update);
  const restored = (await getSession(session.id))!;
  expect(restored.title).toBe(session.title);
  expect(restored.titleIsExplicit).toBe(true);
  expect(restored.blocks[0].draft).toBe(true);
  expect(canReplaceSessionTitle(restored, session.title)).toBe(false);
});

it("surfaces a failed save and lets the same rename retry", async () => {
  let sessions = [chat()];
  const rename = () =>
    renameSession(
      "chat",
      "#646 — Operator session titles",
      () => sessions,
      (update) => {
        sessions = update(sessions);
      },
    );
  vi.mocked(invoke).mockRejectedValueOnce(new Error("disk full"));
  await expect(rename()).rejects.toThrow("disk full");
  await rename();
  expect((await getSession("chat"))?.title).toBe(
    "codex · #646 — Operator session titles",
  );
});

it("rejects missing records and null write results", async () => {
  await expect(
    renameSession("missing", "Title", () => [], vi.fn()),
  ).rejects.toThrow("not found");
  vi.mocked(invoke).mockResolvedValueOnce(null);
  await expect(
    renameSession("chat", "Title", () => [chat()], vi.fn()),
  ).rejects.toThrow("could not be saved");
});
