// @vitest-environment happy-dom
import { act, createElement, StrictMode, useRef, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  newSession,
  type Session,
} from "../../features/sessions/model/session";
import { applyHarnessEvents } from "../../integrations/harness/core/apply";
import { HarnessEventQueue } from "./harnessFlush";
import { updateSessionState } from "./sessionState";

let root: Root;
let container: HTMLDivElement;

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

function mountSessions(strict = false) {
  const blank: Session = {
    ...newSession("claude", "/repo"),
    id: "blank",
    providerAccountId: "default",
  };
  const background: Session = {
    ...newSession("claude", "/repo"),
    id: "background",
    busy: true,
  };
  let snapshot!: {
    sessions: Session[];
    update: (change: (sessions: Session[]) => Session[]) => void;
    rename: (title: string) => void;
    queue: HarnessEventQueue;
    current: () => Session[];
  };

  function Workspace() {
    const [sessions, setSessions] = useState([blank, background]);
    const sessionsRef = useRef(sessions);
    sessionsRef.current = sessions;
    const [queue] = useState(
      () =>
        new HarnessEventQueue(
          () => false,
          (batches) => {
            updateSessionState(sessionsRef, setSessions, (current) =>
              current.map((session) => {
                const events = batches.get(session.id);
                return events ? applyHarnessEvents(session, events) : session;
              }),
            );
          },
        ),
    );
    snapshot = {
      sessions,
      update: (change) => {
        updateSessionState(sessionsRef, setSessions, change);
      },
      rename: (title) => {
        setSessions((current) =>
          current.map((session) =>
            session.id === "blank" ? { ...session, title } : session,
          ),
        );
      },
      queue,
      current: () => sessionsRef.current,
    };
    return null;
  }

  act(() =>
    root.render(
      strict
        ? createElement(StrictMode, null, createElement(Workspace))
        : createElement(Workspace),
    ),
  );
  return () => snapshot;
}

function chooseAccount(sessions: Session[]): Session[] {
  return sessions.map((session) =>
    session.id === "blank"
      ? { ...session, providerAccountId: "account-work" }
      : session,
  );
}

function flushBackground(queue: HarnessEventQueue) {
  queue.enqueue("background", {
    type: "message.delta",
    text: "Still working",
  });
  queue.flush();
}

it("keeps a fresh account choice when another session flushes before render", () => {
  const state = mountSessions();
  act(() => {
    state().update(chooseAccount);
    expect(state().current()[0].providerAccountId).toBe("account-work");
    flushBackground(state().queue);
  });

  expect(state().sessions[0].providerAccountId).toBe("account-work");
  expect(state().sessions[1].blocks[0].text).toBe("Still working");
});

it("keeps background output when the account changes later in the same batch", () => {
  const state = mountSessions();
  act(() => {
    flushBackground(state().queue);
    state().update(chooseAccount);
  });

  expect(state().sessions[0].providerAccountId).toBe("account-work");
  expect(state().sessions[1].blocks[0].text).toBe("Still working");
});

it("preserves other pending React changes while switching accounts and flushing", () => {
  const state = mountSessions();
  act(() => {
    state().rename("Draft title");
    state().update(chooseAccount);
    flushBackground(state().queue);
  });

  expect(state().sessions[0].title).toBe("Draft title");
  expect(state().sessions[0].providerAccountId).toBe("account-work");
  expect(state().sessions[1].blocks[0].text).toBe("Still working");
});

it("retains a newly opened account session during a background flush", () => {
  const state = mountSessions();
  const opened = {
    ...newSession("claude", "/repo"),
    providerAccountId: "account-work",
  };
  act(() => {
    state().update((current) => [...current, opened]);
    flushBackground(state().queue);
  });

  expect(state().sessions).toHaveLength(3);
  expect(state().sessions[2]).toBe(opened);
  expect(state().sessions[0].providerAccountId).toBe("default");
  expect(state().sessions[1].blocks[0].text).toBe("Still working");
});

it("preserves the account and a single copy of streamed output in Strict Mode", () => {
  const state = mountSessions(true);
  act(() => {
    state().rename("Draft title");
    state().update(chooseAccount);
    flushBackground(state().queue);
  });

  expect(state().sessions[0].title).toBe("Draft title");
  expect(state().sessions[0].providerAccountId).toBe("account-work");
  expect(state().sessions[1].blocks).toHaveLength(1);
  expect(state().sessions[1].blocks[0].text).toBe("Still working");
});
