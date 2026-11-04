// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { createMono } from "../features/monos/model/mono";
import App from "./App";
import { abortQuit, handleQuitRequested } from "./model/appLifecycle";

const state = vi.hoisted(() => ({
  sidebar: null as any,
  title: null as any,
  loadMono: vi.fn(),
  save: vi.fn(),
  loadFiles: vi.fn(),
  onClose: null as
    null | ((event: { preventDefault: () => void }) => Promise<void>),
  holdWrites: false,
  writes: [] as Array<() => void>,
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
