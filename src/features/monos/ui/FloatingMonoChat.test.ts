// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { newSession } from "../../sessions/model/session";
import { clearComposerDraft } from "../../sessions/model/draftCache";
import type { FloatingMonoView } from "../model/floatingMono";
import { FloatingMonoChat } from "./FloatingMonoChat";

const native = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
  hide: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ hide: native.hide }),
}));
vi.mock("@tauri-apps/api/webviewWindow", () => ({
  getCurrentWebviewWindow: () => ({ listen: native.listen }),
}));
vi.mock("../../sessions/hooks/useFileDrop", () => ({
  useFileDrop: () => false,
}));
vi.mock("../../sessions/ui/AgentTranscript", () => ({
  AgentTranscript: ({
    blocks,
    onOpenArtifact,
    onOpenPlan,
    onBuildPlan,
  }: {
    blocks: { text: string }[];
    onOpenArtifact: (id: string) => void;
    onOpenPlan?: (id: string) => void;
    onBuildPlan?: (id: string) => void;
  }) =>
    createElement(
      "div",
      null,
      blocks.map((b) => b.text).join(" "),
      createElement(
        "button",
        { "data-open-doc": "", onClick: () => onOpenArtifact("doc-1") },
        "Open document",
      ),
      onOpenPlan &&
        createElement(
          "button",
          { "data-open-plan": "", onClick: () => onOpenPlan("plan-1") },
          "Open plan",
        ),
      onBuildPlan &&
        createElement(
          "button",
          { "data-build-plan": "", onClick: () => onBuildPlan("plan-1") },
          "Build plan",
        ),
    ),
}));
vi.mock("../hooks/useMonoTranscript", () => ({
  useMonoTranscript: (session: { blocks: unknown[] }) => ({
    ...session,
    hasEarlier: false,
    viewingOlderPage: false,
  }),
}));

const monos = [
  {
    id: "first",
    name: "Captain",
    mascot: "crab",
    color: "#aaf",
    sessionId: "chat-a",
  },
  {
    id: "second",
    name: "Scout",
    mascot: "ghost",
    color: "#faa",
    sessionId: "chat-b",
  },
];
function snapshot(index: number): FloatingMonoView {
  return {
    monos,
    monoId: monos[index].id,
    error: null,
    session: {
      ...newSession("codex", "/tmp", "default", "auto"),
      id: monos[index].sessionId,
      blocks: [
        {
          id: "answer",
          role: "assistant",
          text: `${monos[index].name}'s conversation`,
        },
      ],
    },
  };
}
let container: HTMLDivElement;
let root: Root;
let receive: (event: { payload: FloatingMonoView }) => void;
beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      disconnect() {}
    },
  );
  clearComposerDraft("floating:chat-a");
  clearComposerDraft("floating:chat-b");
  native.invoke
    .mockReset()
    .mockImplementation(async (command) =>
      command === "mono_chat_state" ? snapshot(0) : undefined,
    );
  native.listen.mockReset().mockImplementation(async (_event, handler) => {
    receive = handler;
    return () => {};
  });
  native.hide.mockReset().mockResolvedValue(undefined);
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
  vi.unstubAllGlobals();
});
async function render() {
  await act(async () =>
    root.render(createElement(FloatingMonoChat, { onShown: () => {} })),
  );
}

it("shows this window's Mono with its status beneath the name", async () => {
  await render();
  expect(container.textContent).toContain("Captain's conversation");
  expect(container.querySelector("select")).toBeNull();
  const name = container.querySelector("header h1")!;
  expect(name.textContent).toBe("Captain");
  expect(name.nextElementSibling?.getAttribute("data-mono-status")).toBe(
    "idle",
  );
  expect(container.querySelectorAll("[data-mono-status]")).toHaveLength(1);
  expect(
    container.querySelector('[aria-label="Message Captain"]'),
  ).not.toBeNull();
});

it("lists every Mono beside the chat and switches or adds from there", async () => {
  await render();
  const rail = container.querySelector("[data-floating-mono-rail]")!;
  const current = rail.querySelector('[aria-current="true"]');
  expect(current?.getAttribute("aria-label")).toBe("Captain");
  await act(async () =>
    rail.querySelector<HTMLButtonElement>('[aria-label="Scout"]')!.click(),
  );
  expect(native.invoke).toHaveBeenCalledWith("mono_chat_switch", {
    from: "first",
    to: "second",
  });
  await act(async () =>
    rail.querySelector<HTMLButtonElement>('[aria-label="New mono"]')!.click(),
  );
  expect(native.invoke).toHaveBeenCalledWith("mono_chat_action", {
    monoId: "first",
    action: { kind: "create" },
  });
});

it("switches Monos inside the same frame, loading only the conversation", async () => {
  await render();
  const frame = container.querySelector("[data-floating-mono]");
  const rail = container.querySelector("[data-floating-mono-rail]");
  act(() => receive({ payload: { ...snapshot(1), session: null } }));
  expect(container.querySelector("[data-floating-mono]")).toBe(frame);
  expect(container.querySelector("[data-floating-mono-rail]")).toBe(rail);
  expect(container.querySelector("header h1")?.textContent).toBe("Scout");
  expect(
    container.querySelector("[data-floating-mono-loader]")?.textContent,
  ).toBe("Loading conversation…");
  expect(
    rail?.querySelector('[aria-current="true"]')?.getAttribute("aria-label"),
  ).toBe("Scout");
  act(() => receive({ payload: snapshot(1) }));
  expect(container.textContent).toContain("Scout's conversation");
});

it("routes the floating transcript plan controls through the owner", async () => {
  await render();
  await act(async () =>
    container.querySelector<HTMLButtonElement>("[data-open-plan]")!.click(),
  );
  expect(native.invoke).toHaveBeenCalledWith("mono_chat_action", {
    monoId: "first",
    action: { kind: "openPlan", blockId: "plan-1" },
  });
  await act(async () =>
    container.querySelector<HTMLButtonElement>("[data-build-plan]")!.click(),
  );
  expect(native.invoke).toHaveBeenCalledWith("mono_chat_action", {
    monoId: "first",
    action: { kind: "buildPlan", blockId: "plan-1" },
  });
});

it("keeps one loading screen through initial lookup and roster updates until the conversation arrives", async () => {
  let initial!: (view: FloatingMonoView) => void;
  native.invoke.mockImplementation((command) => {
    if (command === "mono_chat_state")
      return new Promise((resolve) => {
        initial = resolve;
      });
    if (command === "mono_chat_ready") {
      // Native presentation is requested after React commits the loader,
      // without waiting on animation frames in a hidden webview.
      expect(
        container.querySelector("[data-floating-mono-loading]"),
      ).not.toBeNull();
      expect(
        container.querySelector("[data-floating-mono-loader]")?.textContent,
      ).toBe("Loading conversation…");
    }
    return Promise.resolve();
  });
  await render();
  const loader = container.querySelector("[data-floating-mono-loading]");
  expect(loader).not.toBeNull();
  expect(native.invoke).toHaveBeenCalledWith("mono_chat_ready");
  expect(container.querySelector("h1")).toBeNull();
  expect(container.querySelector("textarea")).toBeNull();

  const opening = { ...snapshot(0), session: null };
  await act(async () => initial(opening));
  expect(container.querySelector("[data-floating-mono-loading]")).toBe(loader);
  act(() => receive({ payload: { ...opening, monos: [...monos] } }));
  expect(container.querySelector("[data-floating-mono-loading]")).toBe(loader);
  expect(
    container.querySelector("[data-floating-mono-loader]")?.textContent,
  ).toBe("Loading conversation…");

  act(() => receive({ payload: snapshot(0) }));
  expect(container.querySelector("[data-floating-mono-loading]")).toBeNull();
  expect(container.textContent).toContain("Captain's conversation");
  expect(
    container.querySelector('[aria-label="Message Captain"]'),
  ).not.toBeNull();
});

it("shows the empty chat only after a new conversation has finished opening", async () => {
  const empty = snapshot(0);
  empty.session!.blocks = [];
  native.invoke.mockImplementation(async (command) =>
    command === "mono_chat_state" ? { ...empty, session: null } : undefined,
  );
  await render();
  expect(container.textContent).not.toContain("Say hello");
  expect(container.querySelector("[aria-busy=true]")).not.toBeNull();

  act(() => receive({ payload: empty }));
  expect(container.querySelector("[data-floating-mono-loading]")).toBeNull();
  expect(container.textContent).toContain("Say hello to Captain");
  expect(
    container.querySelector('[aria-label="Message Captain"]'),
  ).not.toBeNull();
});

it("shows an opening failure instead of keeping the loader indefinitely", async () => {
  native.invoke.mockImplementation(async (command) => {
    if (command === "mono_chat_state") throw new Error("Workspace unavailable");
  });
  await render();
  expect(container.querySelector('[role="alert"]')?.textContent).toContain(
    "Workspace unavailable",
  );
  expect(container.querySelector('[aria-busy="false"]')).not.toBeNull();
  await act(async () =>
    container
      .querySelector<HTMLButtonElement>('[aria-label="Hide chat"]')!
      .click(),
  );
  expect(native.hide).toHaveBeenCalledOnce();
});

it("keeps the draft when the workspace rejects delivery", async () => {
  await render();
  native.invoke.mockImplementation(async (command) => {
    if (command === "mono_chat_action")
      throw new Error("Workspace unavailable");
  });
  const field = container.querySelector<HTMLTextAreaElement>(
    '[aria-label="Message Captain"]',
  )!;
  const props = Object.entries(field).find(([key]) =>
    key.startsWith("__reactProps"),
  )![1];
  act(() => props.onChange({ target: { value: "Please keep this" } }));
  await act(async () =>
    container
      .querySelector("form")!
      .dispatchEvent(new Event("submit", { bubbles: true, cancelable: true })),
  );
  expect(field.value).toBe("Please keep this");
  expect(container.querySelector('[role="alert"]')?.textContent).toContain(
    "Workspace unavailable",
  );
});

it("ignores a stale initial snapshot after a newer update arrives", async () => {
  let initial!: (view: FloatingMonoView) => void;
  native.invoke.mockImplementation(
    () =>
      new Promise((resolve) => {
        initial = resolve;
      }),
  );
  await render();
  act(() => receive({ payload: snapshot(1) }));
  await act(async () => initial(snapshot(0)));
  expect(container.querySelector("h1")!.textContent).toBe("Scout");
  expect(container.textContent).toContain("Scout's conversation");
});

it("reads a Mono's document in a sheet over the chat and slides it away", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
  native.invoke.mockImplementation(async (command) => {
    if (command === "mono_chat_state") return snapshot(0);
    if (command === "artifacts_get")
      return {
        id: "doc-1",
        kind: "document",
        title: "Menu plan",
        body: "Soup first.",
        createdAt: 1,
        updatedAt: 1,
      };
  });
  await render();
  await act(async () =>
    container.querySelector<HTMLButtonElement>("[data-open-doc]")!.click(),
  );
  const sheet = container.querySelector('[data-artifact-sheet="doc-1"]');
  expect(sheet?.textContent).toContain("Menu plan");
  expect(sheet?.textContent).toContain("Soup first.");
  // Reading in place never asks the main window to take over.
  expect(native.invoke).not.toHaveBeenCalledWith(
    "mono_chat_action",
    expect.anything(),
  );
  act(() => {
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
  });
  expect(container.querySelector("[data-artifact-sheet]")).not.toBeNull();
  act(() => vi.runAllTimers());
  expect(container.querySelector("[data-artifact-sheet]")).toBeNull();
  vi.useRealTimers();
});
