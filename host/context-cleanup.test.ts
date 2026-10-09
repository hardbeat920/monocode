import { afterEach, expect, it } from "vitest";
import {
  existsSync,
  mkdirSync,
  mkdtempSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { HostSession } from "../src/features/connections/model/protocol";
import { HostStore } from "./store";

const cleanups: Array<() => void> = [];
afterEach(() => {
  for (const cleanup of cleanups.splice(0).reverse()) cleanup();
});

function setup() {
  const directory = mkdtempSync(join(tmpdir(), "monocode-context-cleanup-"));
  cleanups.push(() => rmSync(directory, { recursive: true, force: true }));
  const path = join(directory, "host.db");
  const store = new HostStore(path);
  cleanups.push(() => store.close());
  const project = store.addProject(directory, "Test");
  const value: HostSession = {
    projectId: project.id,
    revision: 1,
    status: "idle",
    updatedAt: 1,
    session: {
      id: "deleted-session",
      cwd: directory,
      harness: "codex",
      model: "codex:test",
      modelSettings: {},
      runtimeMode: "supervised",
      title: "Test",
      blocks: [],
    },
  };
  store.transaction(() => store.save(value, { type: "created" }));
  const history = join(directory, "context-history", value.session.id);
  mkdirSync(history, { recursive: true });
  writeFileSync(join(history, "history.md"), "Retained context");
  return { directory, path, store, value, history };
}

it("records cleanup with deletion and recovers a crash before filesystem removal", () => {
  const s = setup();
  s.store.deleteSession(s.value.session.id);
  expect(
    s.store.db
      .prepare("SELECT pending FROM context_history_cleanup WHERE session_id=?")
      .get(s.value.session.id),
  ).toMatchObject({ pending: 1 });
  expect(existsSync(s.history)).toBe(true);
  const reopened = new HostStore(s.path);
  try {
    expect(existsSync(s.history)).toBe(false);
    expect(
      reopened.db
        .prepare(
          "SELECT pending FROM context_history_cleanup WHERE session_id=?",
        )
        .get(s.value.session.id),
    ).toMatchObject({ pending: 0 });
    expect(() =>
      reopened.transaction(() =>
        reopened.save({ ...s.value, revision: 2 }, { type: "late-write" }),
      ),
    ).toThrow("Session was deleted");
    expect(() => reopened.deleteSession(s.value.session.id)).not.toThrow();
  } finally {
    reopened.close();
  }
});

it("rolls back the cleanup obligation when the session deletion fails", () => {
  const s = setup();
  s.store.db.exec(
    "CREATE TRIGGER prevent_delete BEFORE DELETE ON sessions BEGIN SELECT RAISE(ABORT, 'deletion rejected'); END;",
  );
  expect(() => s.store.deleteSession(s.value.session.id)).toThrow(
    "deletion rejected",
  );
  expect(s.store.session(s.value.session.id)).toMatchObject(s.value);
  expect(
    s.store.db
      .prepare("SELECT COUNT(*) AS count FROM context_history_cleanup")
      .get(),
  ).toMatchObject({ count: 0 });
  expect(existsSync(s.history)).toBe(true);
});
