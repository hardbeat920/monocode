import { Profiler } from "react";
import { captureReaderAnchor, readerAnchorTop } from "./readerAnchor";
import { createRoot } from "react-dom/client";
import {
  newSession,
  type Block,
} from "../../src/features/sessions/model/session";
import { SessionStateStore } from "../../src/features/sessions/model/sessionStateStore";
import { newTab } from "../../src/features/workspace/model/layout";
import "../../src/styles/index.css";

// Exercise the real workspace and its shortcut handler. Native services are
// isolated so this benchmark never launches agents or touches saved sessions.
const commands = new Set<string>();
const params = new URLSearchParams(location.search);
const nativeRoot = params.get("nativeRoot");
const projectRoot = nativeRoot ?? "/fixture/project";
const worktreeRoot = nativeRoot ? `${nativeRoot}-wt` : "/fixture/worktrees";
const nativeInternals = (
  window as unknown as {
    __TAURI_INTERNALS__?: {
      invoke: (
        command: string,
        args?: Record<string, unknown>,
      ) => Promise<unknown>;
    };
  }
).__TAURI_INTERNALS__;
const nativeInvoke = nativeRoot
  ? nativeInternals?.invoke.bind(nativeInternals)
  : undefined;
let store: SessionStateStore | undefined;
const update = SessionStateStore.prototype.update;
SessionStateStore.prototype.update = function (...args) {
  store = this;
  return update.apply(this, args);
};
const source = Array.from(
  { length: 80 },
  (_, index) => `export const value${index} = ${index};`,
).join("\n");
const filesFor = (cwd: string) => [
  {
    path: `${cwd}/sample.ts`,
    relative: "sample.ts",
    name: "sample.ts",
    status: "modified",
    additions: 1,
    deletions: 1,
    staged: false,
    unstaged: true,
  },
];
window.addEventListener(
  "keydown",
  (event) => {
    if (event.metaKey && /^[1-9]$/.test(event.key))
      Object.assign(window, { workspaceKeyAt: performance.now() });
  },
  true,
);
const layoutReads: { name: string; duration: number; stack?: string }[] = [];
if (new URLSearchParams(location.search).has("traceLayout")) {
  for (const [prototype, names] of [
    [Element.prototype, ["scrollHeight", "clientHeight", "clientWidth"]],
    [HTMLElement.prototype, ["offsetHeight", "offsetWidth"]],
  ] as const) {
    for (const name of names) {
      const descriptor = Object.getOwnPropertyDescriptor(prototype, name);
      if (!descriptor?.get) continue;
      Object.defineProperty(prototype, name, {
        ...descriptor,
        get() {
          const start = performance.now();
          const result = descriptor.get!.call(this);
          const duration = performance.now() - start;
          if (duration > 2)
            layoutReads.push({ name, duration, stack: new Error().stack });
          return result;
        },
      });
    }
  }
  const measure = Element.prototype.getBoundingClientRect;
  Element.prototype.getBoundingClientRect = function () {
    const start = performance.now();
    const result = measure.call(this);
    const duration = performance.now() - start;
    if (duration > 2)
      layoutReads.push({
        name: `rect:${this.className}`,
        duration,
        stack: new Error().stack,
      });
    return result;
  };
  const focus = HTMLElement.prototype.focus;
  HTMLElement.prototype.focus = function (options) {
    const start = performance.now();
    focus.call(this, options);
    const duration = performance.now() - start;
    if (duration > 2)
      layoutReads.push({
        name: `focus:${this.className}`,
        duration,
        stack: new Error().stack?.split("\n").slice(0, 5).join("\n"),
      });
  };
  const computed = window.getComputedStyle;
  window.getComputedStyle = function (element, pseudo) {
    const style = computed.call(window, element, pseudo);
    return new Proxy(style, {
      get(target, property) {
        const start = performance.now();
        const value = Reflect.get(target, property, target);
        const duration = performance.now() - start;
        if (duration > 2)
          layoutReads.push({
            name: `style:${String(property)}`,
            duration,
            stack: new Error().stack?.split("\n").slice(0, 5).join("\n"),
          });
        return typeof value === "function" ? value.bind(target) : value;
      },
    });
  };
}
const fixtureInvoke = async (
  command: string,
  args: Record<string, unknown> = {},
) => {
  commands.add(command);
  if (
    nativeInvoke &&
    (command.startsWith("plugin:event|") ||
      command === "keybindings_set_overrides")
  )
    return nativeInvoke(command, args);
  if (command === "plugin:event|listen") return 0;
  if (command === "plugin:event|unlisten") return;
  if (command === "control_load") return null;
  if (command === "reminder_list") return [];
  if (command === "reminder_take_open") return null;
  if (
    command === "reminder_configure" ||
    command === "reminder_register_window"
  )
    return;
  if (command === "session_checkpoint_status") return { files: [] };
  if (command === "plugin:window|set_title") return;
  if (command === "set_window_background_blur") return;
  if (
    nativeInvoke &&
    [
      "list_project_files",
      "git_diff_stats",
      "git_diff_index",
      "git_diff_files",
      "git_file_diff",
      "read_text_file",
      "resource_usage",
      "resource_processes",
      "resource_stop_tool",
      "harness_kill",
      "control_turn_finished",
      "quit_poll_reply",
      "quit_decision",
      "quit_ready",
    ].includes(command)
  )
    return nativeInvoke(command, args);
  const cwd = String(args.cwd ?? projectRoot);
  if (command === "list_project_files") return filesFor(cwd);
  if (command === "read_text_file") return source;
  if (command === "git_diff_stats")
    return { files: 1, additions: 1, deletions: 1 };
  if (command === "git_diff_index" || command === "git_diff_files")
    return {
      branch: `worktree-${cwd.split("/").pop()}`,
      head: "fixture",
      files: filesFor(cwd),
      additions: 1,
      deletions: 1,
      ahead: 0,
      behind: 0,
      aheadOfDefault: 0,
      headPushed: true,
      remote: null,
      upstream: null,
      defaultBranch: "main",
    };
  if (command === "git_file_diff")
    return {
      ...filesFor(cwd)[0],
      original: source,
      current: source.replace("value0 = 0", "value0 = 1"),
      binary: false,
      tooLarge: false,
    };
  if (command === "resource_usage")
    return { cpuPercent: 20, memoryPressure: 1 };
  if (command === "list_skills") return [];
  if (command === "git_branch") return "main";
  if (
    command === "autosave_set_enabled" ||
    command === "keybindings_set_overrides"
  )
    return;
  if (command === "quick_composer_take") return null;
  throw new Error(`Unavailable fixture service: ${command}`);
};
Object.assign(window, {
  workspaceLayoutReads: layoutReads,
  workspaceCommands: commands,
  workspaceInvoke: fixtureInvoke,
});
// Tauri's native IPC properties are read-only. The benchmark build aliases
// the public invoke import; browser-only tests install a normal mock bridge.
if (!nativeInvoke)
  Object.assign(window, {
    __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener: () => {} },
    __TAURI_INTERNALS__: {
      metadata: {
        currentWindow: { label: "main" },
        currentWebview: { label: "main" },
      },
      transformCallback: () => 0,
      invoke: fixtureInvoke,
    },
  });
if (nativeInvoke) {
  const showError = (message: string) => {
    const error = document.createElement("pre");
    error.style.cssText =
      "position:fixed;inset:80px 20px;z-index:99999;overflow:auto;background:#222;color:#fff;padding:16px";
    error.textContent = message;
    document.body.append(error);
  };
  window.addEventListener("error", (event) =>
    showError(event.error?.stack ?? event.message),
  );
  window.addEventListener("unhandledrejection", (event) =>
    showError(event.reason?.stack ?? String(event.reason)),
  );
}

localStorage.setItem("monocode.pacedReveal", "0");
localStorage.setItem("monocode.diffViewer", "unified");
localStorage.setItem("monocode.measureNavigation", "1");
localStorage.setItem("monocode.measureStreaming", "1");
const sessions = Array.from({ length: 6 }, (_, index) => {
  const blocks: Block[] = Array.from({ length: 100 }, (_, turn) => [
    {
      id: `${index}-u${turn}`,
      role: "user" as const,
      text: `Inspect module ${turn}.`,
    },
    {
      id: `${index}-a${turn}`,
      role: "assistant" as const,
      text: `## Module ${turn}\n\nReview [sample.ts](sample.ts).\n\n${"This module processes the local sample data and records its result. ".repeat(12)}\n\n\`\`\`typescript\n${Array.from({ length: 25 }, (_, line) => `const value${line} = processItem(${line});`).join("\n")}\n\`\`\``,
    },
  ]).flat();
  return {
    ...newSession("claude", projectRoot),
    id: `workspace-session-${index}`,
    title: `Session ${index + 1}`,
    blocks,
    worktreeCwd: `${worktreeRoot}/${index}`,
  };
});
const tabs = sessions.map((session) => newTab(session.id));
const commits: { duration: number; at: number }[] = [];
Object.assign(window, {
  workspaceCommits: commits,
  workspaceSessions: sessions,
});
const { default: App } = await import("../../src/app/App");
createRoot(document.getElementById("root")!).render(
  <Profiler
    id="workspace"
    onRender={(_id, _phase, duration, _base, _start, at) => {
      commits.push({ duration, at });
    }}
  >
    <App
      resumed={{
        sessions,
        tabs,
        activeTabId: tabs[0].id,
        projectCwd: projectRoot,
      }}
    />
  </Profiler>,
);

// Drive the same queue, reducer and per-session subscriptions as provider output.
// A disposable new/closed tab first captures the real workspace store.
const { HarnessEventQueue } = await import("../../src/app/model/harnessFlush");
const { applyHarnessEvents } = await import("../../src/integrations/harness");
const { noteStreamApplied } =
  await import("../../src/features/sessions/model/streamingPerformance");
const foreground = (id: string) =>
  document
    .querySelector<HTMLElement>(`[data-session-drop="${id}"]`)
    ?.checkVisibility() ?? false;
const queue = new HarnessEventQueue(foreground, (batches) => {
  store?.update(
    (current) =>
      current.map((session) => {
        const events = batches.get(session.id);
        if (!events) return session;
        const next = applyHarnessEvents(session, events);
        noteStreamApplied(
          session.blocks,
          next.blocks,
          events,
          foreground(session.id),
        );
        return next;
      }),
    [...batches.values()].every((events) =>
      events.every((event) => event.type === "message.delta"),
    ),
  );
});
// App flushes its real provider queue in a layout effect on activation. This
// synthetic queue must follow that boundary too, rather than leave the newly
// visible session waiting for its old 250ms background timer.
const activationObserver = new MutationObserver((records) => {
  if (
    records.some((record) =>
      (record.target as Element).hasAttribute("data-workspace-tab"),
    )
  )
    queue.flushForeground();
});
activationObserver.observe(document.body, {
  attributes: true,
  attributeFilter: ["aria-hidden"],
  subtree: true,
});
let timer = 0;
let streamDeadline = 0;
let sequence = 0;
const beginStreams = () => {
  if (!store)
    throw new Error(
      "Open and close a disposable new tab before starting the fixture streams",
    );
  clearInterval(timer);
  clearTimeout(streamDeadline);
  if (nativeInvoke)
    streamDeadline = window.setTimeout(() => finishStreams(), 60_000);
  const run = ++sequence;
  store.update((current) =>
    current.map((session) =>
      sessions.some((entry) => entry.id === session.id)
        ? {
            ...session,
            busy: true,
            blocks: [
              ...session.blocks,
              {
                id: `${session.id}-stream-user-${run}`,
                role: "user",
                text: "Explain the sample while I review adjacent files.",
              },
              {
                id: `${session.id}-stream-answer-${run}`,
                role: "assistant",
                text: "Review [sample.ts](sample.ts).\n\n",
                streaming: true,
              },
            ],
          }
        : session,
    ),
  );
  let tick = 0;
  timer = window.setInterval(() => {
    for (const session of sessions)
      queue.enqueue(session.id, {
        type: "message.delta",
        text:
          tick % 8 === 7
            ? "\n\n```typescript\nconst result = processItem(42);\n```\n\n"
            : "The sample remains visible while this reply grows. ".repeat(3),
      });
    tick++;
  }, 80);
};
const finishStreams = () => {
  clearInterval(timer);
  clearTimeout(streamDeadline);
  for (const session of sessions) {
    queue.enqueue(session.id, { type: "message.completed" });
    queue.enqueue(session.id, { type: "session.ended", code: 0 });
  }
  queue.flush();
  // The real host clears busy in its exit callback after applying final output.
  store?.update((current) =>
    current.map((session) =>
      sessions.some((entry) => entry.id === session.id)
        ? { ...session, busy: false }
        : session,
    ),
  );
};
let frame = 0;
let lastFrame = 0;
const frames: number[] = [];
const sampleFrame = (now: number) => {
  if (lastFrame && frames.length < 10_000) frames.push(now - lastFrame);
  lastFrame = now;
  frame = requestAnimationFrame(sampleFrame);
};
const resetMetrics = () => {
  performance.clearMeasures();
  frames.length = 0;
  lastFrame = 0;
  cancelAnimationFrame(frame);
  frame = requestAnimationFrame(sampleFrame);
};
const metrics = () => ({
  navigation: performance
    .getEntriesByName("monocode:tab-switch")
    .map((entry) => ({
      duration: entry.duration,
      detail: (entry as PerformanceMeasure).detail,
    })),
  streaming: performance
    .getEntriesByName("monocode:stream-visible")
    .map((entry) => ({
      duration: entry.duration,
      detail: (entry as PerformanceMeasure).detail,
    })),
  frames,
});
const saveMetrics = () =>
  nativeInvoke?.("write_text_file", {
    path: "/private/tmp/monocode-native-metrics.json",
    content: JSON.stringify(metrics()),
  });
const startLoad = async () => {
  const binaryPath = params.get("nativeAgent");
  if (!nativeInvoke || !binaryPath)
    throw new Error("A disposable native fixture agent is required");
  for (let index = 0; index < sessions.length; index++)
    await nativeInvoke("harness_spawn", {
      sessionId: sessions[index].id,
      command: binaryPath,
      binaryPath,
      binaryProvider: "claude",
      args: [String(index)],
      cwd: sessions[index].worktreeCwd,
    });
};
Object.assign(window, {
  workspaceBeginStreams: beginStreams,
  workspaceFinishStreams: finishStreams,
  workspaceResetMetrics: resetMetrics,
  workspaceMetrics: metrics,
  workspaceHasStore: () => !!store,
  captureReaderAnchor,
  readerAnchorTop,
});
if (nativeInvoke) {
  // Native automation can finish a run before inspecting its accessibility
  // tree. Inspecting a continuously changing tree can itself stall the test.
  const actions = [
    beginStreams,
    finishStreams,
    resetMetrics,
    saveMetrics,
    startLoad,
  ];
  window.addEventListener(
    "keydown",
    (event) => {
      if (!event.ctrlKey || !event.shiftKey || event.metaKey || event.altKey)
        return;
      const index = /^F(?:[6-9]|10)$/.test(event.key)
        ? Number(event.key.slice(1)) - 6
        : -1;
      if (index < 0) return;
      event.preventDefault();
      event.stopImmediatePropagation();
      void Promise.resolve().then(actions[index]);
    },
    true,
  );
  const controls = document.createElement("div");
  controls.style.cssText =
    "position:fixed;bottom:4px;left:4px;z-index:99999;display:flex;gap:4px;font:11px sans-serif";
  for (const [label, action] of [
    ["Start streams", beginStreams],
    ["Finish streams", finishStreams],
    ["Reset metrics", resetMetrics],
    ["Save metrics", saveMetrics],
    ["Start load (45s)", startLoad],
  ] as const) {
    const button = document.createElement("button");
    button.textContent = label;
    button.style.cssText =
      "padding:3px 6px;background:#333;color:white;border-radius:4px";
    button.onclick = () => {
      void Promise.resolve()
        .then(action)
        .catch((error) => {
          button.textContent = String(error);
        });
    };
    controls.append(button);
  }
  document.body.append(controls);
}
