import { describe, expect, it, vi } from "vitest";
import { newSession } from "../../features/sessions/model/session";
import { persistAfterFailedSessionRemoval } from "./sessionRemovalPersistence";

describe("persistAfterFailedSessionRemoval", () => {
  it("replaces a cleared autosave with the latest live session after abort", () => {
    const session = newSession("cursor", "/tmp/project");
    const latest = { ...session, title: "newer title" };
    const pending = new Map([[session.id, session]]);
    const persist = vi.fn();

    persistAfterFailedSessionRemoval(
      session.id,
      false,
      latest,
      pending,
      persist,
    );

    expect(pending.get(session.id)).toBe(latest);
    expect(persist).toHaveBeenCalledExactlyOnceWith(latest);
  });

  it("requeues the latest live session after the autosave timer already cleared it", () => {
    const session = newSession("cursor", "/tmp/project");
    const latest = { ...session, title: "newer title" };
    const pending = new Map<string, typeof session>();
    const persist = vi.fn();

    persistAfterFailedSessionRemoval(
      session.id,
      false,
      latest,
      pending,
      persist,
    );

    expect(pending.get(session.id)).toBe(latest);
    expect(persist).toHaveBeenCalledExactlyOnceWith(latest);
  });

  it("uses a pending autosave when the session is no longer live", () => {
    const session = newSession("cursor", "/tmp/project");
    const pending = new Map([[session.id, session]]);
    const persist = vi.fn();

    persistAfterFailedSessionRemoval(
      session.id,
      false,
      undefined,
      pending,
      persist,
    );

    expect(pending.get(session.id)).toBe(session);
    expect(persist).toHaveBeenCalledExactlyOnceWith(session);
  });

  it("does not resave a successfully removed session", () => {
    const session = newSession("cursor", "/tmp/project");
    const pending = new Map([[session.id, session]]);
    const persist = vi.fn();

    persistAfterFailedSessionRemoval(
      session.id,
      true,
      session,
      pending,
      persist,
    );

    expect(persist).not.toHaveBeenCalled();
  });
});
