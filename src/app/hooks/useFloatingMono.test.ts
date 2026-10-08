// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import {
  newSession,
  type Session,
} from "../../features/sessions/model/session";
import type {
  FloatingMonoHost,
  FloatingMonoRequest,
} from "../../features/monos/model/floatingMono";
import { useFloatingMono } from "./useFloatingMono";
import { saveMonoMenuBarIcon } from "../../features/settings/model/settings";

const native = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: native.invoke,
  isTauri: () => true,
}));
vi.mock("@tauri-apps/api/webviewWindow", () => ({
  getCurrentWebviewWindow: () => ({ listen: native.listen }),
}));
vi.mock("../../platform/tauri/platform", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  IS_MAC: true,
}));
vi.mock("../../features/monos/model/mono", () => ({
  findMono: (id: string) => ({ id, sessionId: `chat-${id}` }),
  listMonos: () =>
    ["first", "second"].map((id) => ({ id, sessionId: `chat-${id}` })),
  monoLook: () => ({ name: "Mono", mascot: "crab", color: "#aaf" }),
}));

let container: HTMLDivElement;
let root: Root;
let requests: FloatingMonoRequest[];
let sessions: Session[];
let host: FloatingMonoHost;

function Harness({
  sessions,
  enabled = true,
}: {
  sessions: Session[];
  enabled?: boolean;
}) {
  useFloatingMono(sessions, "roster", enabled, host);
  return null;
}

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  sessions = ["first", "second"].map((id) => ({
    ...newSession("codex", "/tmp", "default", "auto"),
    id: `chat-${id}`,
    busy: true,
  }));
  requests = ["first", "second"].map((monoId, index) => ({
    id: index + 1,
    monoId,
    action: { kind: "open" },
  }));
  host = {
    open: vi.fn(async (id) => sessions.find((s) => s.id === `chat-${id}`)),
    submit: vi.fn(),
    stop: vi.fn(),
    approval: vi.fn(),
    question: vi.fn(),
    questionInteraction: vi.fn(),
    reveal: vi.fn(),
    openFile: vi.fn(),
    resume: vi.fn(),
  };
  native.listen.mockReset().mockResolvedValue(() => {});
  native.invoke.mockReset().mockImplementation(async (command) => {
    if (command === "mono_chat_take") {
      const queued = requests;
      requests = [];
      return queued;
    }
    if (command === "mono_chat_accept") return true;
  });
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});

it("streams each open Mono on transcript commits without waiting for a poll", async () => {
  await act(async () => root.render(createElement(Harness, { sessions })));
  const sync = native.invoke.mock.calls.find(
    ([command]) => command === "mono_chat_sync",
  )?.[1];
  const payload = JSON.parse(JSON.stringify(sync));
  expect(
    payload.mascots.map((mascot: { monoId: string }) => mascot.monoId),
  ).toEqual(payload.monos.map((mono: { id: string }) => mono.id));
  for (const mascot of payload.mascots) {
    expect(mascot.rects.length).toBeGreaterThan(0);
    expect(mascot.rects).toContainEqual(
      expect.objectContaining({ fill: "#263331" }),
    );
  }
  expect(host.open).toHaveBeenCalledWith("first");
  expect(host.open).toHaveBeenCalledWith("second");
  native.invoke.mockClear();
  sessions = sessions.map((session, index) => ({
    ...session,
    blocks: [{ id: "reply", role: "assistant", text: `Reply ${index}` }],
  }));
  await act(async () => root.render(createElement(Harness, { sessions })));
  const published = native.invoke.mock.calls.filter(
    ([command]) => command === "mono_chat_publish",
  );
  expect(
    published.map(([, { monoId, session }]) => [
      monoId,
      session.blocks[0].text,
    ]),
  ).toEqual([
    ["first", "Reply 0"],
    ["second", "Reply 1"],
  ]);

  native.invoke.mockClear();
  sessions = sessions.map((session, index) =>
    index
      ? session
      : {
          ...session,
          blocks: [
            { id: "reply", role: "assistant", text: "Reply 0 continues" },
          ],
        },
  );
  await act(async () => root.render(createElement(Harness, { sessions })));
  expect(native.invoke).toHaveBeenCalledExactlyOnceWith("mono_chat_publish", {
    monoId: "first",
    session: sessions[0],
  });
});

it("stops publishing when Monos are disabled", async () => {
  await act(async () => root.render(createElement(Harness, { sessions })));
  native.invoke.mockClear();
  sessions = sessions.map((session) => ({ ...session, busy: false }));
  await act(async () =>
    root.render(createElement(Harness, { sessions, enabled: false })),
  );
  expect(
    native.invoke.mock.calls.some(
      ([command]) => command === "mono_chat_publish",
    ),
  ).toBe(false);
});

it("shows or hides the menu bar icon to match the setting", async () => {
  const stored = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => stored.get(key) ?? null,
    setItem: (key: string, value: string) => stored.set(key, value),
    removeItem: (key: string) => stored.delete(key),
  });
  await act(async () => root.render(createElement(Harness, { sessions })));
  expect(native.invoke).toHaveBeenCalledWith("mono_menu_bar_set_visible", {
    visible: true,
  });
  await act(async () => saveMonoMenuBarIcon(false));
  expect(native.invoke).toHaveBeenLastCalledWith("mono_menu_bar_set_visible", {
    visible: false,
  });
});
