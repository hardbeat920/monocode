// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { createMono, saveMonoSessionId } from "../features/monos/model/mono";
import { newSession } from "../features/sessions/model/session";
import { invoke } from "@tauri-apps/api/core";
import App from "./App";
import {
  abortQuit,
  commitQuit,
  handleQuitRequested,
} from "./model/appLifecycle";

const state = vi.hoisted(() => ({
  sidebar: null as any,
  title: null as any,
  loadMono: vi.fn(),
  save: vi.fn(),
  loadFiles: vi.fn(),
  floating: null as any,
  onClose: null as
    null | ((event: { preventDefault: () => void }) => Promise<void>),
  holdWrites: false,
  writes: [] as Array<() => void>,
}));
vi.mock("../features/monos/ui/MonoOverview", async (load) => {
  const original =
    await load<typeof import("../features/monos/ui/MonoOverview")>();
  return {
    ...original,
    MonoOverview: (props: any) =>
      createElement("div", {
        "data-mono-overview": "",
        "data-document": props.shownDocumentId,
      }),
  };
});
vi.mock("./hooks/useFloatingMono", () => ({
  useFloatingMono: (
    _sessions: unknown,
    _snapshot: unknown,
    _enabled: unknown,
    host: unknown,
  ) => {
    state.floating = host;
  },
}));
vi.mock("./shell/Sidebar", async (load) => {
  const original = await load<typeof import("./shell/Sidebar")>();
  return {
    ...original,
    Sidebar: (props: any) => {
      state.sidebar = props;
      return null;
    },
  };
});
vi.mock("./shell/TitleBar", async (load) => {
  const original = await load<typeof import("./shell/TitleBar")>();
  return {
    ...original,
    TitleBar: (props: any) => {
      state.title = props;
      return null;
    },
  };
});
vi.mock("../features/monos/model/monoWorkspace", async (load) => {
  const original =
    await load<typeof import("../features/monos/model/monoWorkspace")>();
  return { ...original, ensureMonoSession: state.loadMono };
});
vi.mock("../features/monos/model/monoFiles", async (load) => {
  const original =
    await load<typeof import("../features/monos/model/monoFiles")>();
  return {
    ...original,
    loadMonoFiles: state.loadFiles,
    saveMonoFile: state.save,
  };
});
vi.mock("../features/notifications/hooks/useSessionReminders", () => ({
  useSessionReminders: () => ({
    reminders: [],
    due: [],
    dismissDue: vi.fn(),
    schedule: vi.fn(),
    cancel: vi.fn(),
    open: vi.fn(),
    refresh: vi.fn(),
  }),
}));
vi.mock("../features/search/ui/SearchView", () => ({ SearchView: () => null }));
vi.mock("../features/inbox/ui/InboxView", () => ({
  InboxView: () => null,
  LinkedWorkItemPanel: () => null,
}));
vi.mock("../features/mobile/hooks/useMobileWorkspace", () => ({
  useMobileWorkspace: () => {},
}));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string) => {
    if (command === "workspace_set_snapshot" && state.holdWrites)
      return new Promise<void>((resolve) => state.writes.push(resolve));
    return undefined;
  }),
  isTauri: () => false,
  convertFileSrc: (path: string) => path,
}));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () =>
    new Proxy(
      {},
      {
        get: (_target, key) =>
          key === "onCloseRequested"
            ? async (callback: typeof state.onClose) => {
                state.onClose = callback;
                return () => {};
              }
            : vi.fn(async () => () => {}),
      },
    ),
}));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({ onDragDropEvent: vi.fn(async () => () => {}) }),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
  emit: vi.fn(async () => {}),
}));

afterEach(() => {
  document.body.replaceChildren();
  vi.unstubAllGlobals();
  state.holdWrites = false;
  state.writes = [];
  state.onClose = null;
  abortQuit();
});

it("keeps the selected Soul editor and its draft across real workspace navigation and a cancelled Mono load", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
  localStorage.clear();
  const mono = createMono();
  state.loadFiles.mockResolvedValue({
    id: mono.id,
    dir: "/mono",
    soul: "# Original",
    soulHash: "old",
    memory: "",
    memoryHash: "m",
    memoryPath: "/mono/MEMORY.md",
    topics: [],
  });
  state.save.mockResolvedValue("new");
  const node = document.createElement("div");
  document.body.append(node);
  const root = createRoot(node);
  await act(async () => root.render(createElement(App, {})));
  await act(async () => state.sidebar.monos.onEditSoul(mono.id));
  expect(state.loadMono).not.toHaveBeenCalled();
  const editor = document.querySelector<HTMLTextAreaElement>(
    `[data-mono-soul] textarea`,
  )!;
  expect(editor.value).toBe("# Original");
  await act(async () => {
    Object.getOwnPropertyDescriptor(
      HTMLTextAreaElement.prototype,
      "value",
    )!.set!.call(editor, "# Draft");
    editor.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => state.sidebar.onSearch());
  expect(state.sidebar.searchActive).toBe(true);
  expect(editor.value).toBe("# Draft");
  let finish!: (value: any) => void;
  state.loadMono.mockReturnValueOnce(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  await act(async () => state.sidebar.monos.onOpen(mono.id));
  await act(async () => state.title.onOpenInbox());
  await act(async () => finish({ id: "late-session" }));
  expect(state.sidebar.inboxActive).toBe(true);
  expect(document.querySelector("[data-mono-soul] textarea")).toBe(editor);
  expect(editor.value).toBe("# Draft");
  await act(async () =>
    document.querySelector<HTMLButtonElement>('[aria-label="Close"]')!.click(),
  );
  expect(state.save).toHaveBeenCalledWith(mono.id, "soul", "# Draft", "old");
  expect(document.querySelector("[data-mono-soul]")).toBeNull();
  await act(async () => root.unmount());
});

it("holds the editor frozen until overlapping quit and window-close operations finish", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
  localStorage.clear();
  const mono = createMono();
  state.loadFiles.mockResolvedValue({
    id: mono.id,
    dir: "/mono",
    soul: "# Original",
    soulHash: "old",
    memory: "",
    memoryHash: "m",
    memoryPath: "/mono/MEMORY.md",
    topics: [],
  });
  const node = document.createElement("div");
  document.body.append(node);
  const root = createRoot(node);
  await act(async () => root.render(createElement(App, {})));
  await act(async () => state.sidebar.monos.onEditSoul(mono.id));
  state.holdWrites = true;
  let close!: Promise<void>;
  await act(async () => {
    close = state.onClose!({ preventDefault() {} });
  });
  expect(state.writes).toHaveLength(1);
  let quit!: Promise<boolean>;
  await act(async () => {
    quit = handleQuitRequested(8);
  });
  expect(
    document.querySelector("[data-mono-soul]")?.closest("[inert]"),
  ).not.toBeNull();
  await act(async () => {
    state.writes.shift()!();
    await close;
  });
  expect(state.writes).toHaveLength(1);
  expect(
    document.querySelector("[data-mono-soul]")?.closest("[inert]"),
  ).not.toBeNull();
  await act(async () => {
    state.writes.shift()!();
    await quit;
  });
  expect(
    document.querySelector("[data-mono-soul]")?.closest("[inert]"),
  ).not.toBeNull();
  await act(async () => abortQuit(7));
  expect(
    document.querySelector("[data-mono-soul]")?.closest("[inert]"),
  ).not.toBeNull();
  await act(async () => abortQuit(8));
  expect(
    document.querySelector("[data-mono-soul]")?.closest("[inert]"),
  ).toBeNull();
  await act(async () => root.unmount());
});

it("prevents opening an editor while window shutdown is pending", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
  localStorage.clear();
  const mono = createMono();
  const node = document.createElement("div");
  document.body.append(node);
  const root = createRoot(node);
  await act(async () => root.render(createElement(App, {})));
  state.holdWrites = true;
  let close!: Promise<void>;
  await act(async () => {
    close = state.onClose!({ preventDefault() {} });
  });
  expect(state.writes).toHaveLength(1);
  await act(async () => state.sidebar.monos.onEditSoul(mono.id));
  expect(document.querySelector("[data-mono-soul]")).toBeNull();
  await act(async () => {
    state.writes.shift()!();
    await close;
  });
  await act(async () => root.unmount());
});

it("keeps the old Soul draft when replacing it fails", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
  localStorage.clear();
  const first = createMono();
  const second = createMono();
  state.loadFiles.mockResolvedValue({
    id: first.id,
    dir: "/mono",
    soul: "# Original",
    soulHash: "old",
    memory: "",
    memoryHash: "m",
    memoryPath: "/mono/MEMORY.md",
    topics: [],
  });
  let fail!: (error: Error) => void;
  state.save.mockReset().mockReturnValueOnce(
    new Promise((_resolve, reject) => {
      fail = reject;
    }),
  );
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  const node = document.createElement("div");
  document.body.append(node);
  const root = createRoot(node);
  await act(async () => root.render(createElement(App, {})));
  await act(async () => state.sidebar.monos.onEditSoul(first.id));
  const editor = document.querySelector<HTMLTextAreaElement>(
    "[data-mono-soul] textarea",
  )!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(
      HTMLTextAreaElement.prototype,
      "value",
    )!.set!.call(editor, "# First draft");
    editor.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => state.sidebar.monos.onEditSoul(second.id));
  expect(state.save).toHaveBeenCalledWith(
    first.id,
    "soul",
    "# First draft",
    "old",
  );
  await act(async () => fail(new Error("write failed")));
  expect(document.querySelector("[data-mono-soul] textarea")).toBe(editor);
  expect(editor.value).toBe("# First draft");
  expect(document.querySelector('[role="alert"]')?.textContent).toContain(
    "Could not save",
  );
  warn.mockRestore();
  await act(async () => root.unmount());
});

it("shows an error when the selected Soul cannot be loaded", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
  localStorage.clear();
  const mono = createMono();
  state.loadFiles.mockReset().mockRejectedValue(new Error("unavailable"));
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  const node = document.createElement("div");
  document.body.append(node);
  const root = createRoot(node);
  await act(async () => root.render(createElement(App, {})));
  await act(async () => state.sidebar.monos.onEditSoul(mono.id));
  expect(document.querySelector('[role="alert"]')?.textContent).toContain(
    "Could not load the Soul",
  );
  await act(async () => root.unmount());
  warn.mockRestore();
});

it("does not open an artifact after its Mono load is superseded", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
  localStorage.clear();
  const first = createMono();
  const other = createMono();
  const session = newSession("codex", "/work");
  let finish!: (value: unknown) => void;
  const deferred = new Promise((resolve) => {
    finish = resolve;
  });
  state.loadMono
    .mockReset()
    .mockImplementationOnce(async (id: string, host: any) => {
      saveMonoSessionId(id, session.id);
      host.add(session);
      return session;
    })
    .mockReturnValueOnce(deferred)
    .mockResolvedValue(session)
    .mockResolvedValue(session);
  const node = document.createElement("div");
  document.body.append(node);
  const root = createRoot(node);
  await act(async () => root.render(createElement(App, {})));
  await act(async () => state.sidebar.monos.onOpen(first.id));
  expect(state.sidebar.monos.activeId).toBe(first.id);
  let opening!: Promise<void>;
  await act(async () => {
    opening = state.floating.openArtifact(other.id, "stale-artifact");
  });
  await act(async () => state.sidebar.monos.onOpen(first.id));
  await act(async () => {
    const stale = newSession("codex", "/other");
    saveMonoSessionId(other.id, stale.id);
    finish(stale);
    await opening;
  });
  expect(
    document
      .querySelector("[data-mono-overview]")
      ?.getAttribute("data-document"),
  ).toBeNull();
  await act(async () =>
    state.floating.openArtifact(first.id, "current-artifact"),
  );
  expect(
    document
      .querySelector("[data-mono-overview]")
      ?.getAttribute("data-document"),
  ).toBe("current-artifact");
  await act(async () => root.unmount());
});

it("retains the regular settings Soul draft across navigation and refuses shutdown after a failed save", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
  localStorage.clear();
  const mono = createMono();
  state.loadFiles.mockReset().mockResolvedValue({
    id: mono.id,
    dir: "/mono",
    soul: "# Original",
    soulHash: "old",
    memory: "",
    memoryHash: "m",
    memoryPath: "/mono/MEMORY.md",
    topics: [],
  });
  state.loadMono
    .mockReset()
    .mockImplementation(async (id: string, host: any) => {
      const session = newSession("codex", "/work");
      saveMonoSessionId(id, session.id);
      host.add(session);
      return session;
    });
  state.save.mockReset().mockRejectedValueOnce(new Error("disk failed"));
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  const node = document.createElement("div");
  document.body.append(node);
  const root = createRoot(node);
  await act(async () => root.render(createElement(App, {})));
  await act(async () => state.sidebar.monos.onOpen(mono.id));
  expect(state.title.onOpenMonoSettings).toBeTypeOf("function");
  await act(async () => state.title.onOpenMonoSettings());
  const soulButton = [
    ...document.querySelectorAll<HTMLButtonElement>(
      "[data-mono-settings] nav button",
    ),
  ].find((button) => button.textContent === "Soul")!;
  await act(async () => soulButton.click());
  const editor = document.querySelector<HTMLTextAreaElement>(
    "[data-mono-settings] [data-mono-soul] textarea",
  )!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(
      HTMLTextAreaElement.prototype,
      "value",
    )!.set!.call(editor, "# Regular draft");
    editor.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => state.title.onOpenInbox());
  expect(state.sidebar.inboxActive).toBe(true);
  expect(
    document.querySelector("[data-mono-settings] [data-mono-soul] textarea"),
  ).toBe(editor);
  let quit!: boolean;
  await act(async () => {
    quit = await handleQuitRequested(91);
  });
  expect(quit).toBe(false);
  expect(state.save).toHaveBeenCalledWith(
    mono.id,
    "soul",
    "# Regular draft",
    "old",
  );
  expect(editor.value).toBe("# Regular draft");
  expect(
    document.querySelector('[data-mono-settings] [role="alert"]')?.textContent,
  ).toContain("Could not save");
  await act(async () => root.unmount());
  warn.mockRestore();
});

it("does not allow a timed-out quit to discard a pending Soul save", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
  localStorage.clear();
  const mono = createMono();
  state.loadFiles.mockReset().mockResolvedValue({
    id: mono.id,
    dir: "/mono",
    soul: "# Original",
    soulHash: "old",
    memory: "",
    memoryHash: "m",
    memoryPath: "/mono/MEMORY.md",
    topics: [],
  });
  let finishSave!: (hash: string) => void;
  state.save
    .mockReset()
    .mockReturnValueOnce(
      new Promise((resolve) => {
        finishSave = resolve;
      }),
    )
    .mockResolvedValueOnce("newer");
  const node = document.createElement("div");
  document.body.append(node);
  const root = createRoot(node);
  await act(async () => root.render(createElement(App, {})));
  await act(async () => state.sidebar.monos.onEditSoul(mono.id));
  const editor = document.querySelector<HTMLTextAreaElement>(
    "[data-mono-soul] textarea",
  )!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(
      HTMLTextAreaElement.prototype,
      "value",
    )!.set!.call(editor, "# Pending");
    editor.dispatchEvent(new Event("input", { bubbles: true }));
  });
  let quit!: Promise<void>;
  await act(async () => {
    quit = commitQuit(101);
  });
  expect(editor.closest("[inert]")).not.toBeNull();
  await act(async () => abortQuit(101));
  expect(editor.closest("[inert]")).toBeNull();
  await act(async () => {
    await quit;
    expect(invoke).toHaveBeenCalledWith("quit_ready", {
      id: 101,
      persisted: false,
    });
  });
  await act(async () => {
    Object.getOwnPropertyDescriptor(
      HTMLTextAreaElement.prototype,
      "value",
    )!.set!.call(editor, "# After abort");
    editor.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => finishSave("new"));
  expect(state.save).toHaveBeenLastCalledWith(
    mono.id,
    "soul",
    "# After abort",
    "new",
  );
  expect(editor.closest("[inert]")).toBeNull();
  expect(editor.value).toBe("# After abort");
  await act(async () => root.unmount());
});

it("freezes a regular Soul draft during replacement and keeps it if saving fails", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
  localStorage.clear();
  const first = createMono();
  const second = createMono();
  state.loadFiles.mockReset().mockResolvedValue({
    id: first.id,
    dir: "/mono",
    soul: "# Original",
    soulHash: "old",
    memory: "",
    memoryHash: "m",
    memoryPath: "/mono/MEMORY.md",
    topics: [],
  });
  state.loadMono
    .mockReset()
    .mockImplementation(async (id: string, host: any) => {
      const session = newSession("codex", "/work");
      saveMonoSessionId(id, session.id);
      host.add(session);
      return session;
    });
  let fail!: (error: Error) => void;
  state.save.mockReset().mockReturnValueOnce(
    new Promise((_resolve, reject) => {
      fail = reject;
    }),
  );
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  const node = document.createElement("div");
  document.body.append(node);
  const root = createRoot(node);
  await act(async () => root.render(createElement(App, {})));
  await act(async () => state.sidebar.monos.onOpen(first.id));
  await act(async () => state.title.onOpenMonoSettings());
  const soul = [
    ...document.querySelectorAll<HTMLButtonElement>(
      "[data-mono-settings] nav button",
    ),
  ].find((button) => button.textContent === "Soul")!;
  await act(async () => soul.click());
  const editor = document.querySelector<HTMLTextAreaElement>(
    "[data-mono-settings] [data-mono-soul] textarea",
  )!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(
      HTMLTextAreaElement.prototype,
      "value",
    )!.set!.call(editor, "# Draft");
    editor.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => state.sidebar.monos.onOpen(second.id));
  await act(async () => state.title.onOpenMonoSettings());
  expect(editor.closest("[inert]")).not.toBeNull();
  await act(async () => fail(new Error("write failed")));
  expect(
    document.querySelector("[data-mono-settings] [data-mono-soul] textarea"),
  ).toBe(editor);
  expect(editor.value).toBe("# Draft");
  expect(editor.closest("[inert]")).toBeNull();
  await act(async () => root.unmount());
  warn.mockRestore();
});

it("does not reopen old settings after a pending new Mono load when settings were closed", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
  localStorage.clear();
  const first = createMono();
  state.loadFiles.mockReset().mockResolvedValue({
    id: first.id,
    dir: "/mono",
    soul: "# Original",
    soulHash: "old",
    memory: "",
    memoryHash: "m",
    memoryPath: "/mono/MEMORY.md",
    topics: [],
  });
  let finish!: () => void;
  state.loadMono
    .mockReset()
    .mockImplementationOnce(async (id: string, host: any) => {
      const session = newSession("codex", "/work");
      saveMonoSessionId(id, session.id);
      host.add(session);
      return session;
    })
    .mockImplementationOnce(
      (id: string, host: any) =>
        new Promise((resolve) => {
          finish = () => {
            const session = newSession("codex", "/work");
            saveMonoSessionId(id, session.id);
            host.add(session);
            resolve(session);
          };
        }),
    );
  const node = document.createElement("div");
  document.body.append(node);
  const root = createRoot(node);
  await act(async () => root.render(createElement(App, {})));
  await act(async () => state.sidebar.monos.onOpen(first.id));
  await act(async () => state.title.onOpenMonoSettings());
  await act(async () => state.sidebar.monos.onCreate());
  expect(finish).toBeTypeOf("function");
  const dialog = document
    .querySelector("[data-mono-settings]")!
    .closest('[role="dialog"]')!;
  await act(async () =>
    dialog.querySelector<HTMLButtonElement>('[aria-label="Close"]')!.click(),
  );
  await act(async () => finish());
  expect(document.querySelector("[data-mono-settings]")).toBeNull();
  await act(async () => root.unmount());
});

it("does not let a delayed close of old settings close its replacement", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
  localStorage.clear();
  const first = createMono();
  const second = createMono();
  state.loadFiles.mockReset().mockResolvedValue({
    id: first.id,
    dir: "/mono",
    soul: "# Original",
    soulHash: "old",
    memory: "",
    memoryHash: "m",
    memoryPath: "/mono/MEMORY.md",
    topics: [],
  });
  state.loadMono
    .mockReset()
    .mockImplementation(async (id: string, host: any) => {
      const session = newSession("codex", "/work");
      saveMonoSessionId(id, session.id);
      host.add(session);
      return session;
    });
  let finish!: (hash: string) => void;
  state.save.mockReset().mockReturnValueOnce(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  const node = document.createElement("div");
  document.body.append(node);
  const root = createRoot(node);
  await act(async () => root.render(createElement(App, {})));
  await act(async () => state.sidebar.monos.onOpen(first.id));
  await act(async () => state.title.onOpenMonoSettings());
  const soul = [
    ...document.querySelectorAll<HTMLButtonElement>(
      "[data-mono-settings] nav button",
    ),
  ].find((button) => button.textContent === "Soul")!;
  await act(async () => soul.click());
  const editor = document.querySelector<HTMLTextAreaElement>(
    "[data-mono-settings] [data-mono-soul] textarea",
  )!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(
      HTMLTextAreaElement.prototype,
      "value",
    )!.set!.call(editor, "# Draft");
    editor.dispatchEvent(new Event("input", { bubbles: true }));
  });
  await act(async () => state.sidebar.monos.onOpen(second.id));
  await act(async () => state.title.onOpenMonoSettings());
  const dialog = document
    .querySelector("[data-mono-settings]")!
    .closest('[role="dialog"]')!;
  await act(async () =>
    dialog.querySelector<HTMLButtonElement>('[aria-label="Close"]')!.click(),
  );
  await act(async () => finish("saved"));
  expect(document.querySelector("[data-mono-settings]")).not.toBeNull();
  expect(
    document.querySelector("[data-mono-settings] [data-mono-soul]"),
  ).toBeNull();
  await act(async () => root.unmount());
});

it("flushes both Soul editors and cancels quit when either save fails", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
  localStorage.clear();
  const first = createMono();
  const second = createMono();
  state.loadFiles.mockReset().mockResolvedValue({
    id: first.id,
    dir: "/mono",
    soul: "# Original",
    soulHash: "old",
    memory: "",
    memoryHash: "m",
    memoryPath: "/mono/MEMORY.md",
    topics: [],
  });
  state.loadMono
    .mockReset()
    .mockImplementation(async (id: string, host: any) => {
      const session = newSession("codex", "/work");
      saveMonoSessionId(id, session.id);
      host.add(session);
      return session;
    });
  state.save.mockReset().mockImplementation(async (id: string) => {
    if (id === first.id) throw new Error("regular save failed");
    return "saved";
  });
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  const node = document.createElement("div");
  document.body.append(node);
  const root = createRoot(node);
  await act(async () => root.render(createElement(App, {})));
  await act(async () => state.sidebar.monos.onOpen(first.id));
  await act(async () => state.title.onOpenMonoSettings());
  const soul = [
    ...document.querySelectorAll<HTMLButtonElement>(
      "[data-mono-settings] nav button",
    ),
  ].find((button) => button.textContent === "Soul")!;
  await act(async () => soul.click());
  const regular = document.querySelector<HTMLTextAreaElement>(
    "[data-mono-settings] [data-mono-soul] textarea",
  )!;
  await act(async () => state.sidebar.monos.onEditSoul(second.id));
  const editors = [
    ...document.querySelectorAll<HTMLTextAreaElement>(
      "[data-mono-soul] textarea",
    ),
  ];
  const direct = editors.find((editor) => editor !== regular)!;
  const edit = (editor: HTMLTextAreaElement, text: string) => {
    Object.getOwnPropertyDescriptor(
      HTMLTextAreaElement.prototype,
      "value",
    )!.set!.call(editor, text);
    editor.dispatchEvent(new Event("input", { bubbles: true }));
  };
  await act(async () => {
    edit(regular, "# Regular");
    edit(direct, "# Direct");
  });
  let quit!: boolean;
  await act(async () => {
    quit = await handleQuitRequested(111);
  });
  expect(quit).toBe(false);
  expect(state.save).toHaveBeenCalledWith(first.id, "soul", "# Regular", "old");
  expect(state.save).toHaveBeenCalledWith(second.id, "soul", "# Direct", "old");
  expect(regular.value).toBe("# Regular");
  expect(direct.value).toBe("# Direct");
  await act(async () => root.unmount());
  warn.mockRestore();
});

it("still closes regular settings after reopening the same Mono settings", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
  localStorage.clear();
  const mono = createMono();
  state.loadFiles.mockReset().mockResolvedValue({
    id: mono.id,
    dir: "/mono",
    soul: "# Original",
    soulHash: "old",
    memory: "",
    memoryHash: "m",
    memoryPath: "/mono/MEMORY.md",
    topics: [],
  });
  state.loadMono
    .mockReset()
    .mockImplementation(async (id: string, host: any) => {
      const session = newSession("codex", "/work");
      saveMonoSessionId(id, session.id);
      host.add(session);
      return session;
    });
  const node = document.createElement("div");
  document.body.append(node);
  const root = createRoot(node);
  await act(async () => root.render(createElement(App, {})));
  await act(async () => state.sidebar.monos.onOpen(mono.id));
  await act(async () => state.title.onOpenMonoSettings());
  await act(async () => state.title.onOpenMonoSettings());
  const dialog = document
    .querySelector("[data-mono-settings]")!
    .closest('[role="dialog"]')!;
  await act(async () =>
    dialog.querySelector<HTMLButtonElement>('[aria-label="Close"]')!.click(),
  );
  expect(document.querySelector("[data-mono-settings]")).toBeNull();
  await act(async () => root.unmount());
});

it("immediately unfreezes both editors when an aborted quit has a never-settling save", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
  localStorage.clear();
  const first = createMono();
  const second = createMono();
  state.loadFiles.mockReset().mockResolvedValue({
    id: first.id,
    dir: "/mono",
    soul: "# Original",
    soulHash: "old",
    memory: "",
    memoryHash: "m",
    memoryPath: "/mono/MEMORY.md",
    topics: [],
  });
  state.loadMono
    .mockReset()
    .mockImplementation(async (id: string, host: any) => {
      const session = newSession("codex", "/work");
      saveMonoSessionId(id, session.id);
      host.add(session);
      return session;
    });
  state.save
    .mockReset()
    .mockImplementation((id: string) =>
      id === first.id ? new Promise(() => {}) : Promise.resolve("direct-saved"),
    );
  const node = document.createElement("div");
  document.body.append(node);
  const root = createRoot(node);
  await act(async () => root.render(createElement(App, {})));
  await act(async () => state.sidebar.monos.onOpen(first.id));
  await act(async () => state.title.onOpenMonoSettings());
  const soul = [
    ...document.querySelectorAll<HTMLButtonElement>(
      "[data-mono-settings] nav button",
    ),
  ].find((button) => button.textContent === "Soul")!;
  await act(async () => soul.click());
  const regular = document.querySelector<HTMLTextAreaElement>(
    "[data-mono-settings] [data-mono-soul] textarea",
  )!;
  await act(async () => state.sidebar.monos.onEditSoul(second.id));
  const direct = [
    ...document.querySelectorAll<HTMLTextAreaElement>(
      "[data-mono-soul] textarea",
    ),
  ].find((editor) => editor !== regular)!;
  const edit = (editor: HTMLTextAreaElement, text: string) => {
    Object.getOwnPropertyDescriptor(
      HTMLTextAreaElement.prototype,
      "value",
    )!.set!.call(editor, text);
    editor.dispatchEvent(new Event("input", { bubbles: true }));
  };
  await act(async () => {
    edit(regular, "# Regular");
    edit(direct, "# Direct");
  });
  let quit!: Promise<void>;
  await act(async () => {
    quit = commitQuit(201);
  });
  expect(regular.closest("[inert]")).not.toBeNull();
  expect(direct.closest("[inert]")).not.toBeNull();
  await act(async () => {
    abortQuit(201);
    await quit;
  });
  expect(invoke).toHaveBeenCalledWith("quit_ready", {
    id: 201,
    persisted: false,
  });
  expect(regular.closest("[inert]")).toBeNull();
  expect(direct.closest("[inert]")).toBeNull();
  await act(async () => {
    edit(regular, "# Still editable");
    edit(direct, "# Also editable");
  });
  expect(regular.value).toBe("# Still editable");
  expect(direct.value).toBe("# Also editable");
  await act(async () => root.unmount());
});

it("keeps a newer quit frozen when an older aborted save completes late", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
  localStorage.clear();
  const mono = createMono();
  state.loadFiles.mockReset().mockResolvedValue({
    id: mono.id,
    dir: "/mono",
    soul: "# Original",
    soulHash: "old",
    memory: "",
    memoryHash: "m",
    memoryPath: "/mono/MEMORY.md",
    topics: [],
  });
  let finishSave!: (hash: string) => void;
  state.save
    .mockReset()
    .mockReturnValueOnce(
      new Promise((resolve) => {
        finishSave = resolve;
      }),
    )
    .mockResolvedValueOnce("newer");
  const node = document.createElement("div");
  document.body.append(node);
  const root = createRoot(node);
  await act(async () => root.render(createElement(App, {})));
  await act(async () => state.sidebar.monos.onEditSoul(mono.id));
  const editor = document.querySelector<HTMLTextAreaElement>(
    "[data-mono-soul] textarea",
  )!;
  const edit = (text: string) => {
    Object.getOwnPropertyDescriptor(
      HTMLTextAreaElement.prototype,
      "value",
    )!.set!.call(editor, text);
    editor.dispatchEvent(new Event("input", { bubbles: true }));
  };
  await act(async () => edit("# First draft"));
  let firstQuit!: Promise<void>;
  await act(async () => {
    firstQuit = commitQuit(301);
  });
  await act(async () => {
    abortQuit(301);
    await firstQuit;
  });
  await act(async () => edit("# Newer draft"));
  state.holdWrites = true;
  let secondQuit!: Promise<void>;
  await act(async () => {
    secondQuit = commitQuit(302);
  });
  await act(async () => abortQuit(301));
  expect(editor.closest("[inert]")).not.toBeNull();
  await act(async () => finishSave("first-saved"));
  expect(state.save).toHaveBeenLastCalledWith(
    mono.id,
    "soul",
    "# Newer draft",
    "first-saved",
  );
  expect(state.writes).toHaveLength(1);
  expect(editor.closest("[inert]")).not.toBeNull();
  await act(async () => abortQuit(302));
  expect(editor.closest("[inert]")).toBeNull();
  await act(async () => {
    state.writes.shift()!();
    await secondQuit;
  });
  expect(editor.value).toBe("# Newer draft");
  await act(async () => root.unmount());
});

it("keeps the window-close lock when an overlapping quit is aborted", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
  localStorage.clear();
  const mono = createMono();
  state.loadFiles
    .mockReset()
    .mockResolvedValue({
      id: mono.id,
      dir: "/mono",
      soul: "# Original",
      soulHash: "old",
      memory: "",
      memoryHash: "m",
      memoryPath: "/mono/MEMORY.md",
      topics: [],
    });
  let finish!: (hash: string) => void;
  state.save.mockReset().mockReturnValueOnce(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  const node = document.createElement("div");
  document.body.append(node);
  const root = createRoot(node);
  await act(async () => root.render(createElement(App, {})));
  await act(async () => state.sidebar.monos.onEditSoul(mono.id));
  const editor = document.querySelector<HTMLTextAreaElement>(
    "[data-mono-soul] textarea",
  )!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(
      HTMLTextAreaElement.prototype,
      "value",
    )!.set!.call(editor, "# Pending");
    editor.dispatchEvent(new Event("input", { bubbles: true }));
  });
  state.holdWrites = true;
  let close!: Promise<void>;
  await act(async () => {
    close = state.onClose!({ preventDefault() {} });
  });
  let quit!: Promise<void>;
  await act(async () => {
    quit = commitQuit(401);
  });
  await act(async () => {
    abortQuit(401);
    await quit;
  });
  expect(editor.closest("[inert]")).not.toBeNull();
  await act(async () => finish("saved"));
  expect(state.writes).toHaveLength(1);
  expect(editor.closest("[inert]")).not.toBeNull();
  await act(async () => {
    state.writes.shift()!();
    await close;
  });
  expect(editor.closest("[inert]")).toBeNull();
  await act(async () => root.unmount());
});

it("keeps edits and permits retry after the aborted quit's write fails late", async () => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
  localStorage.clear();
  const mono = createMono();
  state.loadFiles
    .mockReset()
    .mockResolvedValue({
      id: mono.id,
      dir: "/mono",
      soul: "# Original",
      soulHash: "old",
      memory: "",
      memoryHash: "m",
      memoryPath: "/mono/MEMORY.md",
      topics: [],
    });
  let fail!: (error: Error) => void;
  state.save
    .mockReset()
    .mockReturnValueOnce(
      new Promise((_resolve, reject) => {
        fail = reject;
      }),
    )
    .mockResolvedValueOnce("saved");
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  const node = document.createElement("div");
  document.body.append(node);
  const root = createRoot(node);
  await act(async () => root.render(createElement(App, {})));
  await act(async () => state.sidebar.monos.onEditSoul(mono.id));
  const editor = document.querySelector<HTMLTextAreaElement>(
    "[data-mono-soul] textarea",
  )!;
  const edit = (text: string) => {
    Object.getOwnPropertyDescriptor(
      HTMLTextAreaElement.prototype,
      "value",
    )!.set!.call(editor, text);
    editor.dispatchEvent(new Event("input", { bubbles: true }));
  };
  await act(async () => edit("# First"));
  let quit!: Promise<void>;
  await act(async () => {
    quit = commitQuit(501);
  });
  await act(async () => {
    abortQuit(501);
    await quit;
  });
  await act(async () => edit("# Newer"));
  await act(async () => fail(new Error("disk failed")));
  expect(editor.value).toBe("# Newer");
  const retry = [
    ...document.querySelectorAll<HTMLButtonElement>("button"),
  ].find((button) => button.textContent === "Retry")!;
  await act(async () => retry.click());
  expect(state.save).toHaveBeenLastCalledWith(
    mono.id,
    "soul",
    "# Newer",
    "old",
  );
  expect(editor.value).toBe("# Newer");
  await act(async () => root.unmount());
  warn.mockRestore();
});
