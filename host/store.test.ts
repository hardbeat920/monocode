import { execFileSync } from "node:child_process";
import { lstatSync, mkdirSync, mkdtempSync, renameSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, it, vi } from "vitest";
import type { HostSession } from "../src/features/connections/model/protocol";
import { HostStore } from "./store";

function fixture() {
  const directory = mkdtempSync(join(tmpdir(), "monocode-removal-recovery-"));
  const path = join(directory, "copy");
  mkdirSync(path);
  execFileSync("git", ["init", path], { stdio: "ignore" });
  const database = join(directory, "host.db");
  const store = new HostStore(database);
  const project = store.addProject(directory, "Test");
  const value: HostSession = {
    projectId: project.id,
    revision: 1,
    status: "idle",
    createdAt: 10,
    updatedAt: 20,
    archived: true,
    session: {
      id: "owner",
      cwd: directory,
      worktreeCwd: path,
      cowId: "copy-id",
      branch: "mc/test",
      providerSessionId: "provider-thread",
      harness: "codex",
      model: "codex:test",
      runtimeMode: "supervised",
      title: "Keep my transcript",
      blocks: [{ id: "message", role: "user", text: "Keep my work" }],
    },
  };
  store.transaction(() => store.save(value, { type: "created" }));
  const metadata = lstatSync(path, { bigint: true });
  const identity: [string, string] = [String(metadata.dev), String(metadata.ino)];
  return { directory, path, database, store, value, identity };
}

it.each(["surviving", "removed", "replaced root", "replaced git", "reattached", "deleted"])(
  "recovers an interrupted CoW removal with a %s checkout/session", (state) => {
    const { directory, path, database, value, store, identity } = fixture();
    let reopened: HostStore | undefined;
    try {
      expect(() => store.prepareCowRemoval("copy-id", path, directory, false, identity)).toThrow("Sessions still use");
      expect(() => store.prepareCowRemoval("copy-id", path, directory, true, ["-1", "-1"])).toThrow("checkout was replaced");
      store.prepareCowRemoval("copy-id", path, directory, true, identity);
      expect(store.session("owner").session.worktreeRemoved).toBe(true);
      if (state === "reattached") {
        const current = store.session("owner");
        store.transaction(() => store.save({
          ...current,
          revision: current.revision + 1,
          session: { ...current.session, worktreeRemoved: false, worktreeCwd: undefined, providerSessionId: "new-thread" },
        }, { type: "reattached" }));
      } else if (state === "deleted") store.deleteSession("owner");
      store.close();
      if (state === "removed") rmSync(path, { recursive: true });
      if (state === "replaced root") {
        renameSync(path, `${path}-old`);
        mkdirSync(path);
        execFileSync("git", ["init", path], { stdio: "ignore" });
      }
      if (state === "replaced git") {
        renameSync(join(path, ".git"), join(path, "git-old"));
        execFileSync("git", ["init", path], { stdio: "ignore" });
      }
      reopened = new HostStore(database);
      expect(reopened.db.prepare("SELECT COUNT(*) AS count FROM workspace_removals").get()!.count).toBe(0);
      if (state === "deleted") expect(() => reopened!.session("owner")).toThrow("Session not found");
      else {
        const restored = reopened.session("owner");
        expect(restored.archived).toBe(true);
        expect(restored.session.blocks).toEqual(value.session.blocks);
        if (state === "surviving") expect(restored.session).toEqual(value.session);
        else if (state === "reattached") expect(restored.session).toMatchObject({ worktreeRemoved: false, providerSessionId: "new-thread" });
        else {
          expect(restored.session).toMatchObject({ cwd: directory, worktreeRemoved: true });
          expect(restored.session.cowId).toBeUndefined();
          expect(restored.session.providerSessionId).toBeUndefined();
        }
      }
    } finally {
      reopened?.close();
      try { store.close(); } catch { /* Already closed to simulate process exit. */ }
      rmSync(directory, { recursive: true, force: true });
    }
  },
);

it("journals atomically and keeps deleted checkouts detached when cleanup fails", () => {
  const { directory, path, database, store, identity } = fixture();
  const errorLog = vi.spyOn(console, "error").mockImplementation(() => {});
  let reopened: HostStore | undefined;
  try {
    store.db.exec(`CREATE TRIGGER fail_journal BEFORE INSERT ON workspace_removals
      BEGIN SELECT RAISE(ABORT, 'journal unavailable'); END;`);
    expect(() => store.prepareCowRemoval("copy-id", path, directory, true, identity)).toThrow("journal unavailable");
    expect(store.session("owner").session.worktreeRemoved).toBeUndefined();
    store.db.exec("DROP TRIGGER fail_journal");
    store.prepareCowRemoval("copy-id", path, directory, true, identity);
    rmSync(path, { recursive: true });
    store.db.exec(`CREATE TRIGGER fail_cleanup BEFORE DELETE ON workspace_removals
      BEGIN SELECT RAISE(ABORT, 'cleanup unavailable'); END;`);
    expect(() => store.finishCowRemoval(path, true)).toThrow("cleanup unavailable");
    expect(store.session("owner").session.worktreeRemoved).toBe(true);
    store.close();
    reopened = new HostStore(database);
    expect(errorLog).toHaveBeenCalled();
    expect(reopened.session("owner").session.worktreeRemoved).toBe(true);
    expect(reopened.db.prepare("SELECT COUNT(*) AS count FROM workspace_removals").get()!.count).toBe(1);
    reopened.db.exec("DROP TRIGGER fail_cleanup");
    expect(reopened.finishCowRemoval(path, true)).toBe(true);
    expect(reopened.db.prepare("SELECT COUNT(*) AS count FROM workspace_removals").get()!.count).toBe(0);
  } finally {
    errorLog.mockRestore();
    reopened?.close();
    try { store.close(); } catch { /* Already closed to simulate process exit. */ }
    rmSync(directory, { recursive: true, force: true });
  }
});
