// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import {
  SessionPane,
  type SessionPaneProps,
} from "../../sessions/ui/SessionPane";
import type { Block, Session } from "../../sessions/model/session";
import type { AgentModel } from "../../sessions/model/models";
import { clearComposerDraft } from "../../sessions/model/draftCache";
import { rememberRemoteProject } from "../model/remoteProjects";
import { preloadRemoteSession } from "./RemoteSession";
import { rememberRemoteSession, remoteSessionFor } from "../model/connections";
import "../model/remoteCommands";
import type {
  HostCommand,
  HostDescriptor,
  HostModelCatalog,
  HostSession,
  RemoteMachine,
} from "../model/protocol";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), isTauri: () => false }));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({ onDragDropEvent: async () => () => {} }),
}));
vi.mock("../../sessions/ui/AgentTranscript", () => ({
  AgentTranscript: ({
    blocks,
    busy,
    onSendDraft,
    onRemoveDraft,
    onOpenFile,
    onOpenDiff,
  }: {
    blocks: Block[];
    busy: boolean;
    onSendDraft?: (block: Block) => void;
    onRemoveDraft?: (block: Block) => void;
    onOpenFile?: (path: string) => void;
    onOpenDiff?: (path: string) => void;
  }) =>
    createElement(
      "ol",
      { "aria-label": "Transcript", "data-busy": busy },
      createElement("button", { "aria-label": "Open transcript file", onClick: () => onOpenFile?.("src/app.ts") }),
      createElement("button", { "aria-label": "Open transcript diff", onClick: () => onOpenDiff?.("src/app.ts") }),
      blocks.map((block) =>
        createElement(
          "li",
          { key: block.id },
          block.text,
          block.draft && onSendDraft
            ? createElement(
                "button",
                {
                  "aria-label": "Send remote draft",
                  onClick: () => onSendDraft(block),
                },
                "Send draft",
              )
            : null,
          block.draft && onRemoveDraft
            ? createElement(
                "button",
                {
                  "aria-label": "Remove remote draft",
                  onClick: () => onRemoveDraft(block),
                },
                "Remove draft",
              )
            : null,
        ),
      ),
    ),
}));

const machine: RemoteMachine = {
  id: "machine",
  name: "Home server",
  endpoint: "ssh://me@home",
  environmentId: "env",
};
const effort = (id: string, value: string) => ({
  id,
  label: "Reasoning",
  kind: "select" as const,
  value,
  options: ["low", "medium", "high"].map((option) => ({
    value: option,
    label: option[0].toUpperCase() + option.slice(1),
  })),
});
const gpt: AgentModel = {
  id: "codex:gpt-test",
  harness: "codex",
  name: "GPT Test",
  nativeId: "gpt-test",
  settings: [effort("reasoningEffort", "medium")],
};
const cursor: AgentModel = {
  id: "cursor:composer-test",
  harness: "cursor",
  name: "Composer Test",
  nativeId: "composer-test",
  settings: [],
};

let root: Root;
let container: HTMLDivElement;
let host: HostSession | undefined;
let catalog: HostModelCatalog | Error;
let providers: HostDescriptor["providers"];
let capabilities: string[];
let commands: HostCommand[];
let projectKey: string;
let syncDelay: Promise<void> | undefined;
let dispatchDelay: Promise<void> | undefined;
let branchFailure: string | undefined;
let branchActionFailure: string | undefined;
let currentBranch: string;
let createdBranch: string | undefined;
let createdWorktree: string | undefined;
let deletedSessions: string[];

beforeEach(() => {
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  localStorage.clear();
  clearComposerDraft("shell");
  localStorage.setItem("monocode.modelControls", "beside");
  commands = [];
  host = undefined;
  syncDelay = undefined;
  dispatchDelay = undefined;
  branchFailure = undefined;
  branchActionFailure = undefined;
  currentBranch = "main";
  createdBranch = undefined;
  createdWorktree = undefined;
  deletedSessions = [];
  catalog = { models: { codex: [gpt] }, errors: {} };
  providers = ["codex"];
  capabilities = ["attachments.upload", "sessions.plan", "sessions.draft"];
  projectKey = rememberRemoteProject("env", {
    id: "project",
    name: "repo",
    cwd: "/home/me/repo",
  }).key;
  vi.mocked(invoke).mockReset();
  vi.mocked(invoke).mockImplementation(async (command, input) => {
    if (command === "remote_machines") return [machine];
    if (command !== "remote_request") return undefined;
    const { method, params } = input as {
      method: string;
      params: HostCommand & { sessionId?: string };
    };
    const workspace = method === "workspace.run"
      ? params as unknown as { command: string; args: Record<string, unknown> }
      : undefined;
    const operation = workspace?.command ?? method;
    const commandParams = workspace?.args ?? params;
    if (method === "environment.describe")
      return {
        protocolVersion: 1,
        environmentId: "env",
        name: "home",
        providers,
        capabilities,
      };
    if (method === "models.list") {
      if (catalog instanceof Error) throw catalog.message;
      return catalog;
    }
    if (operation === "git.branches" || operation === "git_branches") {
      if (branchFailure) throw new Error(branchFailure);
      if (workspace) return {
        current: currentBranch,
        detached: false,
        branches: ["main", "dev", ...(createdBranch ? [createdBranch] : [])]
          .map((name) => ({ name, current: name === currentBranch, remote: null })),
      };
      return {
        current: currentBranch,
        branches: ["main", "dev", ...(createdBranch ? [createdBranch] : [])],
      };
    }
    if (operation === "git.worktrees" || operation === "git_worktrees")
      return {
        defaultRoot: "/home/me/repo-worktrees",
        worktrees: [
          {
            path: "/home/me/repo",
            branch: "main",
            head: "abc",
            isMain: true,
            missing: false,
          },
          {
            path: "/home/me/repo-worktrees/dev",
            branch: "dev",
            head: "def",
            isMain: false,
            missing: false,
          },
          ...(createdWorktree
            ? [
                {
                  path: createdWorktree,
                  branch: createdBranch,
                  head: "abc",
                  isMain: false,
                  missing: false,
                },
              ]
            : []),
        ],
      };
    if (method === "git.worktreeCreate") {
      createdBranch = params.branch;
      createdWorktree = `/home/me/repo-worktrees/wt-${params.branch.replace(/[^a-zA-Z0-9_-]/g, "-")}`;
      return {
        path: createdWorktree,
        branch: createdBranch,
        head: "abc",
        isMain: false,
        missing: false,
      };
    }
    if (operation === "git.switch" || operation === "git.createBranch" ||
        operation === "git_checkout" || operation === "git_create_branch") {
      if (branchActionFailure) throw new Error(branchActionFailure);
      currentBranch = String(operation.startsWith("git_") ? commandParams.name : params.branch);
      if (operation === "git.createBranch" || operation === "git_create_branch") createdBranch = currentBranch;
      if (workspace) return currentBranch;
      return {
        current: params.branch,
        branches: ["main", "dev", ...(createdBranch ? [createdBranch] : [])],
      };
    }
    if (method === "sessions.sync") {
      if (syncDelay) await syncDelay;
      return { kind: "snapshot", value: host };
    }
    if (method === "attachments.upload")
      return { offset: (params as { size: number }).size };
    if (method === "commands.dispatch") {
      if (dispatchDelay) await dispatchDelay;
      return dispatch(params);
    }
    if (method === "sessions.delete") {
      deletedSessions.push(params.sessionId!);
      host = undefined;
      return { deleted: true };
    }
    throw new Error(`Unexpected method ${method}`);
  });
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});
afterEach(() => {
  act(() => root.unmount());
  container.remove();
  document.body.innerHTML = "";
  vi.unstubAllGlobals();
  localStorage.clear();
});

/** A minimal host engine: persists commands the way the real one does. */
function dispatch(command: HostCommand) {
  commands.push(command);
  if (command.type === "create") {
    host = {
      projectId: command.projectId,
      revision: 1,
      status: "idle",
      updatedAt: 0,
      session: {
        id: "host-session",
        cwd: command.worktreeCwd ?? "/home/me/repo",
        harness: command.harness,
        model: command.model,
        modelSettings: command.modelSettings ?? {},
        runtimeMode: command.runtimeMode,
        title: "New remote session",
        blocks: [],
      },
    };
  } else if (host && command.type === "confirmProviderInspection") {
    host = {
      ...host, revision: host.revision + 1,
      session: { ...host.session, pendingSwitch: undefined,
        providerContext: { ...host.session.providerContext!, delivery: undefined } },
    };
  } else if (host && (command.type === "configure" || command.type === "switchProvider")) {
    host = {
      ...host,
      revision: host.revision + 1,
      session: {
        ...host.session,
        ...(command.type === "switchProvider" ? { harness: command.harness } : {}),
        model: command.model,
        modelSettings: command.modelSettings,
        runtimeMode: command.runtimeMode,
      },
    };
  } else if (host && command.type === "send") {
    host = {
      ...host,
      revision: host.revision + 1,
      status: "idle",
      session: {
        ...host.session,
        blocks: [
          ...host.session.blocks.filter(
            (block) => block.id !== command.draftBlockId,
          ),
          { id: command.commandId, role: "user", text: command.text },
          { id: `${command.commandId}-reply`, role: "assistant", text: "Done" },
        ],
      },
    };
  } else if (host && command.type === "draft") {
    host = {
      ...host,
      revision: host.revision + 1,
      session: {
        ...host.session,
        blocks: [
          ...host.session.blocks,
          {
            id: command.commandId,
            role: "user",
            text: command.text,
            draft: true,
          },
        ],
      },
    };
  } else if (host && command.type === "removeDraft") {
    host = {
      ...host,
      revision: host.revision + 1,
      session: {
        ...host.session,
        blocks: host.session.blocks.filter(
          (block) => block.id !== command.draftBlockId,
        ),
      },
    };
  }
  return {
    commandId: command.commandId,
    sessionId: host?.session.id ?? "host-session",
    revision: host?.revision ?? 1,
  };
}

const shell = (): Session => ({
  id: "shell",
  cwd: projectKey,
  harness: "claude",
  model: "claude:sonnet-5",
  modelSettings: { reasoningEffort: "high" },
  runtimeMode: "supervised",
  title: "New session",
  blocks: [],
});

async function render(
  session = shell(),
  extra: Partial<SessionPaneProps> = {},
) {
  const props = {
    session,
    visible: true,
    focused: true,
    inSplit: false,
    composerFocused: false,
    recents: [],
    onFocus: vi.fn(),
    onClose: vi.fn(),
    ...extra,
  } as unknown as SessionPaneProps;
  await act(async () => root.render(createElement(SessionPane, props)));
  await settle();
}
async function settle() {
  for (let i = 0; i < 5; i++)
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
}
const byLabel = (prefix: string) =>
  container.querySelector<HTMLButtonElement>(`button[aria-label^="${prefix}"]`);
async function type(text: string) {
  const textarea = container.querySelector("textarea")!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(
      HTMLTextAreaElement.prototype,
      "value",
    )!.set!.call(textarea, text);
    textarea.dispatchEvent(new Event("input", { bubbles: true }));
  });
}
async function send(text: string) {
  await type(text);
  await act(async () => byLabel("Send")!.click());
  await settle();
}
async function chooseEffort(label: string) {
  await act(async () => byLabel("Reasoning:")!.click());
  const option = [
    ...document.body.querySelectorAll<HTMLButtonElement>(
      '[role="menuitemradio"]',
    ),
  ].find((item) => item.textContent?.includes(label))!;
  await act(async () => option.click());
  await settle();
}

async function openProviderModels(provider: string) {
  await act(async () => byLabel("Codex GPT Test")!.click());
  const tab = document.body.querySelector<HTMLButtonElement>(`[role="tab"][aria-label="${provider}"]`);
  if (tab) await act(async () => tab.click());
  await settle();
  return tab;
}

it("uses the normal composer with the host branch in its top row", async () => {
  await render();
  expect(container.querySelector("textarea")).not.toBeNull();
  // The machine is named in the project rail, not the composer.
  expect(container.textContent).not.toContain("Home server");
  expect(byLabel("Branch main")).not.toBeNull();
  expect(
    [...container.querySelectorAll("button")].some(
      (button) => button.textContent === "Changes",
    ),
  ).toBe(false);
  // The host's model, keeping the tab's effort where the model supports it.
  expect(byLabel("Reasoning:")?.getAttribute("aria-label")).toBe(
    "Reasoning: High",
  );
  expect(container.textContent).toContain("GPT Test");
  // Nothing from the old standalone remote view or local-only tools.
  expect(container.textContent).not.toContain("New remote session");
  expect(container.textContent).not.toContain("Apply settings");
  expect(byLabel("Add files or choose a mode")?.closest(".hidden")).toBeNull();
  await act(async () => byLabel("Add files or choose a mode")!.click());
  expect(document.body.textContent).toContain("Upload file");
  expect(document.body.textContent).toContain("Plan mode");
  expect(document.body.textContent).toContain("Draft");
  expect(document.body.textContent).not.toContain("Operator");
  expect(byLabel("Project ")).toBeNull();
});

it("opens transcript files and diffs through the shared remote tabs", async () => {
  const onOpenFile = vi.fn();
  const onOpenDiff = vi.fn();
  await render(shell(), { onOpenFile, onOpenDiff });
  await send("Inspect files");
  await act(async () => byLabel("Open transcript file")!.click());
  await act(async () => byLabel("Open transcript diff")!.click());
  expect(onOpenFile).toHaveBeenCalledWith("remote://env/home/me/repo/src/app.ts");
  expect(onOpenDiff).toHaveBeenCalledWith("remote://env/home/me/repo/src/app.ts");
});

it("opens a host conversation in an already mounted empty tab", async () => {
  await render();
  dispatch({
    type: "create",
    commandId: "existing-session",
    projectId: "project",
    harness: "codex",
    model: gpt.id,
    runtimeMode: "supervised",
  });
  host = {
    ...host!,
    session: {
      ...host!.session,
      title: "Codex · Existing conversation",
      blocks: [{ id: "old-message", role: "user", text: "Earlier message" }],
    },
  };
  commands = [];
  await act(async () => rememberRemoteSession("shell", "host-session"));
  await settle();
  expect(container.textContent).toContain("Earlier message");
  await send("Continue here");
  expect(commands.some((command) => command.type === "create")).toBe(false);
  expect(commands.some((command) => command.type === "send")).toBe(true);
});

it("keeps an unopened remote conversation docked while its transcript loads", async () => {
  dispatch({
    type: "create",
    commandId: "existing-session",
    projectId: "project",
    harness: "codex",
    model: gpt.id,
    runtimeMode: "supervised",
  });
  host = {
    ...host!,
    session: {
      ...host!.session,
      id: "unopened-session",
      blocks: [{ id: "old-message", role: "user", text: "Earlier message" }],
    },
  };
  rememberRemoteSession("shell", "unopened-session");
  let releaseSync = () => {};
  syncDelay = new Promise<void>((resolve) => {
    releaseSync = resolve;
  });

  await render();
  const composer = container.querySelector("[data-session-composer]");
  expect(composer?.classList.contains("max-w-4xl")).toBe(true);
  expect(container.textContent).not.toContain("Loading conversation…");
  expect(container.textContent).not.toContain("What should we work on?");

  await act(async () => {
    releaseSync();
    syncDelay = undefined;
  });
  await settle();
  expect(container.textContent).toContain("Earlier message");
  expect(container.querySelector("[data-session-composer]")).toBe(composer);
});

it("shows a preloaded conversation's transcript on its first render", async () => {
  dispatch({
    type: "create",
    commandId: "existing-session",
    projectId: "project",
    harness: "codex",
    model: gpt.id,
    runtimeMode: "supervised",
  });
  host = {
    ...host!,
    session: {
      ...host!.session,
      id: "preloaded-session",
      blocks: [{ id: "old-message", role: "user", text: "Earlier message" }],
    },
  };
  await preloadRemoteSession(machine.id, "preloaded-session");
  rememberRemoteSession("shell", "preloaded-session");
  // Hold every later sync: what shows must come from the preload alone.
  syncDelay = new Promise<void>(() => {});

  await act(async () =>
    root.render(
      createElement(SessionPane, {
        session: shell(),
        visible: true,
        focused: true,
        inSplit: false,
        composerFocused: false,
        recents: [],
        onFocus: vi.fn(),
        onClose: vi.fn(),
      } as unknown as SessionPaneProps),
    ),
  );
  expect(container.textContent).toContain("Earlier message");
  expect(container.textContent).not.toContain("What should we work on?");
});

it("shows an unavailable branch when Git lookup fails", async () => {
  branchFailure = "fatal: not a git repository";
  await render();
  const picker = byLabel("No git repository");
  expect(picker).not.toBeNull();
  expect((picker as HTMLButtonElement).disabled).toBe(true);
});

it("creates the host session with the chosen settings on the first message", async () => {
  await render();
  await chooseEffort("Medium");
  expect(commands).toHaveLength(0);
  await send("Fix the tests");
  expect(commands.map((command) => command.type)).toEqual(["create", "send"]);
  expect(commands[0]).toMatchObject({
    projectId: "project",
    harness: "codex",
    model: "codex:gpt-test",
    modelSettings: { reasoningEffort: "medium" },
  });
  expect(commands[1]).toMatchObject({
    sessionId: "host-session",
    text: "Fix the tests",
  });
  expect(remoteSessionFor("shell")).toBe("host-session");
  expect(container.textContent).toContain("Fix the tests");
});

it("keeps a new session on its selected remote provider", async () => {
  providers = ["codex", "cursor"];
  catalog = {
    models: { codex: [gpt], cursor: [cursor] },
    errors: {},
  };
  await render({
    ...shell(),
    harness: "cursor",
    model: cursor.id,
    modelSettings: {},
  });
  await send("Use Cursor remotely");
  expect(commands[0]).toMatchObject({
    type: "create",
    harness: "cursor",
    model: cursor.id,
  });
});

it("drops settings from the tab that the host's model does not offer", async () => {
  await render({
    ...shell(),
    harness: "codex",
    model: "codex:gpt-test",
    modelSettings: { reasoningEffort: "high", serviceTier: "fast" },
  });
  await send("Fix the tests");
  expect(commands[0]).toMatchObject({ type: "create", model: "codex:gpt-test" });
  expect(commands[0]).toHaveProperty("modelSettings", {
    reasoningEffort: "high",
  });
});

it("sends a remote plan turn from the plus menu", async () => {
  await render();
  await act(async () => byLabel("Add files or choose a mode")!.click());
  const plan = [
    ...document.body.querySelectorAll<HTMLButtonElement>("button"),
  ].find((button) => button.textContent?.includes("Plan mode"))!;
  await act(async () => plan.click());
  await send("Plan the migration");
  expect(commands.at(-1)).toMatchObject({
    type: "send",
    text: "Plan the migration",
    intent: "plan",
  });
});

async function saveDraft(text: string) {
  await act(async () => byLabel("Add files or choose a mode")!.click());
  const draft = [
    ...document.body.querySelectorAll<HTMLButtonElement>("button"),
  ].find((button) =>
    button.textContent?.includes("Save this message without starting"),
  )!;
  await act(async () => draft.click());
  await type(text);
  await act(async () => byLabel("Save draft")!.click());
  await settle();
}
const transcriptItems = (text: string) =>
  [...container.querySelectorAll("ol[aria-label='Transcript'] li")].filter(
    (item) => item.textContent?.includes(text),
  );

it("saves and sends a remote draft", async () => {
  await render();
  await saveDraft("Review this later");
  expect(commands.map((command) => command.type)).toEqual(["create", "draft"]);
  expect(commands.at(-1)).toMatchObject({
    type: "draft",
    text: "Review this later",
  });
  expect(byLabel("Send remote draft")).not.toBeNull();
  await act(async () => byLabel("Send remote draft")!.click());
  await settle();
  expect(commands.at(-1)).toMatchObject({
    type: "send",
    text: "Review this later",
    draftBlockId: commands[1].commandId,
  });
});

it("keeps a new draft on screen while the host confirms it", async () => {
  await render();
  let releaseSync = () => {};
  syncDelay = new Promise<void>((resolve) => {
    releaseSync = resolve;
  });
  await saveDraft("Review this later");
  expect(commands.map((command) => command.type)).toEqual(["create", "draft"]);
  expect(transcriptItems("Review this later")).toHaveLength(1);
  expect(container.textContent).not.toContain("What should we work on");
  expect(container.querySelector('[aria-label="Transcript"]')?.getAttribute("data-busy")).toBe("false");
  expect(byLabel("Send remote draft")).not.toBeNull();
  expect(byLabel("Remove remote draft")).not.toBeNull();
  await act(async () => {
    releaseSync();
    syncDelay = undefined;
  });
  await settle();
  expect(transcriptItems("Review this later")).toHaveLength(1);
});

it("replaces a sent draft with its message at once", async () => {
  await render();
  await saveDraft("Review this later");
  syncDelay = new Promise<void>(() => {});
  await act(async () => byLabel("Send remote draft")!.click());
  expect(commands.at(-1)).toMatchObject({ type: "send" });
  expect(transcriptItems("Review this later")).toHaveLength(1);
  expect(byLabel("Send remote draft")).toBeNull();
});

it("removes a draft-only conversation with its draft, as a local one", async () => {
  await render();
  await saveDraft("Review this later");
  const sessionId = remoteSessionFor("shell");
  expect(sessionId).toBe("host-session");
  await act(async () => byLabel("Remove remote draft")!.click());
  expect(transcriptItems("Review this later")).toHaveLength(0);
  await settle();
  expect(deletedSessions).toEqual([sessionId]);
  expect(commands.some((command) => command.type === "removeDraft")).toBe(
    false,
  );
  expect(remoteSessionFor("shell")).toBeUndefined();
  expect(container.textContent).toContain("What should we work on");
});

it("keeps a sent message visible until the host sync confirms it", async () => {
  await render();
  await send("First");
  let releaseSync = () => {};
  syncDelay = new Promise<void>((resolve) => {
    releaseSync = resolve;
  });
  await type("Second");
  await act(async () => byLabel("Send")!.click());
  expect(commands.at(-1)).toMatchObject({ type: "send", text: "Second" });
  const secondMessages = () =>
    [...container.querySelectorAll("ol[aria-label='Transcript'] li")].filter(
      (item) => item.textContent === "Second",
    );
  expect(secondMessages()).toHaveLength(1);
  expect(
    container
      .querySelector("ol[aria-label='Transcript']")
      ?.getAttribute("data-busy"),
  ).toBe("true");
  await act(async () => {
    releaseSync();
    syncDelay = undefined;
  });
  await settle();
  expect(secondMessages()).toHaveLength(1);
});

it("does not flash a status banner while an ordinary message is in flight", async () => {
  await render();
  await send("First");
  let releaseDispatch = () => {};
  dispatchDelay = new Promise<void>((resolve) => {
    releaseDispatch = resolve;
  });

  await type("Second");
  await act(async () => byLabel("Send")!.click());
  expect(container.textContent).toContain("Second");
  expect(container.textContent).not.toContain("Waiting for the host to confirm");

  await act(async () => {
    releaseDispatch();
    dispatchDelay = undefined;
  });
  await settle();
  expect(commands.at(-1)).toMatchObject({ type: "send", text: "Second" });
});

it("keeps the first turn active while its accepted message awaits host sync", async () => {
  await render();
  let releaseSync = () => {};
  syncDelay = new Promise<void>((resolve) => {
    releaseSync = resolve;
  });
  await send("First remote turn");
  expect(commands.map((command) => command.type)).toEqual(["create", "send"]);
  const transcript = () =>
    container.querySelector("ol[aria-label='Transcript']");
  expect(transcript()?.textContent).toContain("First remote turn");
  expect(transcript()?.getAttribute("data-busy")).toBe("true");
  await act(async () => {
    releaseSync();
    syncDelay = undefined;
  });
  await settle();
  expect(transcript()?.querySelectorAll("li")).toHaveLength(2);
  expect(transcript()?.getAttribute("data-busy")).toBe("false");
});

it("starts a remote session in the worktree chosen before its first message", async () => {
  await render();
  await act(async () => byLabel("Workspace Current checkout")!.click());
  const existing = [
    ...document.body.querySelectorAll<HTMLButtonElement>("button"),
  ].find((button) => button.textContent?.trim() === "Existing worktree…");
  await act(async () => existing!.click());
  await settle();
  const worktree = [
    ...document.body.querySelectorAll<HTMLButtonElement>('[role="menuitem"]'),
  ].find((button) => button.title === "remote://env/home/me/repo-worktrees/dev");
  expect(worktree).toBeDefined();
  await act(async () => worktree!.click());
  await send("Work in dev");
  expect(commands[0]).toMatchObject({
    type: "create",
    worktreeCwd: "/home/me/repo-worktrees/dev",
  });
  expect(host?.session.cwd).toBe("/home/me/repo-worktrees/dev");
  expect(container.querySelector('[aria-label="Workspace Worktree"]')?.tagName)
    .toBe("DIV");
  expect(byLabel("Workspace Worktree")).toBeNull();
});

it("creates a host worktree through the composer and selects it", async () => {
  await render();
  await act(async () => byLabel("Workspace Current checkout")!.click());
  expect(
    document.body.querySelector('[aria-label="Workspace"]'),
  ).not.toBeNull();
  expect(document.body.textContent).toContain("Existing worktree…");
  expect(document.body.textContent).not.toContain("Worktree settings");
  expect(
    document.body.querySelector('[aria-label="Existing worktrees"]'),
  ).toBeNull();
  expect(
    document.body.querySelector('input[placeholder="Search working copies…"]'),
  ).toBeNull();
  const create = [
    ...document.body.querySelectorAll<HTMLButtonElement>("button"),
  ].find((button) => button.textContent?.trim() === "New worktree");
  await act(async () => create!.click());
  expect(byLabel("Workspace New worktree")).not.toBeNull();
  expect(byLabel("Create worktree from main")).not.toBeNull();
  expect(byLabel("Branch main")).toBeNull();
  await act(async () => byLabel("Create worktree from main")!.click());
  const devBase = [
    ...document.body.querySelectorAll<HTMLButtonElement>('[role="option"]'),
  ].find((button) => button.textContent?.trim() === "dev");
  await act(async () => devBase!.click());
  expect(byLabel("Create worktree from dev")).not.toBeNull();
  expect(invoke).not.toHaveBeenCalledWith(
    "remote_request",
    expect.objectContaining({ method: "git.worktreeCreate" }),
  );
  await send("Work in new tree");
  expect(invoke).toHaveBeenCalledWith(
    "remote_request",
    expect.objectContaining({
      method: "git.worktreeCreate",
      params: expect.objectContaining({
        projectId: "project",
        cwd: "/home/me/repo",
        branch: expect.stringMatching(/^mc\/[a-z0-9]+$/),
        base: "dev",
        existing: false,
      }),
    }),
  );
  expect(commands[0]).toMatchObject({
    type: "create",
    worktreeCwd: createdWorktree,
    autoWorktreeBranch: createdBranch,
  });
});

it("searches and creates a host branch from the composer picker", async () => {
  await render();
  await act(async () => byLabel("Branch main")!.click());
  const search = document.body.querySelector<HTMLInputElement>(
    'input[aria-label="Search or create a branch"]',
  )!;
  await act(async () => {
    Object.getOwnPropertyDescriptor(
      HTMLInputElement.prototype,
      "value",
    )!.set!.call(search, "feature/test");
    search.dispatchEvent(new Event("input", { bubbles: true }));
  });
  const create = [
    ...document.body.querySelectorAll<HTMLButtonElement>("button"),
  ].find((button) =>
    button.textContent?.includes("Create and checkout feature/test"),
  );
  await act(async () => create!.click());
  await settle();
  expect(invoke).toHaveBeenCalledWith(
    "remote_request",
    expect.objectContaining({
      method: "workspace.run",
      params: expect.objectContaining({
        command: "git_create_branch",
        args: expect.objectContaining({
          name: "feature/test",
          cwd: "/home/me/repo",
        }),
      }),
    }),
  );
  expect(byLabel("Branch feature/test")).not.toBeNull();
});

it("keeps a failed host branch action in the picker", async () => {
  branchActionFailure =
    "Commit or stash changes on the host before switching branches";
  await render();
  await act(async () => byLabel("Branch main")!.click());
  const dev = [
    ...document.body.querySelectorAll<HTMLButtonElement>('[role="option"]'),
  ].find((button) => button.textContent?.includes("dev"));
  await act(async () => dev!.click());
  await settle();
  expect(
    document.body.querySelector('[aria-label="Branch picker"]'),
  ).not.toBeNull();
  expect(document.body.textContent).toContain(branchActionFailure);
});

it("locks a started remote session to its worktree like a local session", async () => {
  host = {
    projectId: "project",
    revision: 1,
    status: "idle",
    updatedAt: 0,
    session: {
      id: "host-session",
      cwd: "/home/me/repo",
      harness: "codex",
      model: "codex:gpt-test",
      modelSettings: {},
      runtimeMode: "supervised",
      title: "Existing conversation",
      blocks: [{ id: "first", role: "user", text: "Earlier work" }],
    },
  };
  rememberRemoteSession("shell", "host-session");
  await render();
  expect(container.querySelector('[aria-label="Workspace Current checkout"]')?.tagName)
    .toBe("DIV");
  expect(byLabel("Workspace Current checkout")).toBeNull();
  expect(byLabel("Branch main")).not.toBeNull();
  expect(commands).toHaveLength(0);
});

it("applies effort changes directly and uses them on the next turn", async () => {
  await render();
  await send("First");
  await chooseEffort("Low");
  expect(commands.at(-1)).toMatchObject({
    type: "configure",
    model: "codex:gpt-test",
    modelSettings: { reasoningEffort: "low" },
  });
  expect(host?.session.modelSettings).toEqual({ reasoningEffort: "low" });
  await settle();
  expect(
    commands.filter((command) => command.type === "configure"),
  ).toHaveLength(1);
  await send("Second");
  expect(commands.at(-1)).toMatchObject({ type: "send", text: "Second" });

  // Reopening the tab shows the saved setting.
  await act(async () => root.unmount());
  root = createRoot(container);
  await render();
  expect(byLabel("Reasoning:")?.getAttribute("aria-label")).toBe(
    "Reasoning: Low",
  );
});

it("continues a started remote session with another provider when the host supports it", async () => {
  capabilities.push("sessionProviderSwitchV1");
  providers = ["codex", "cursor"];
  catalog = { models: { codex: [gpt], cursor: [cursor] }, errors: {} };
  await render();
  await send("First");
  await vi.waitFor(() => expect(host?.session.blocks).toHaveLength(2));
  const revision = host!.revision;
  expect(await openProviderModels("Cursor")).not.toBeNull();
  const option = document.body.querySelector<HTMLElement>('[role="option"][aria-label^="Composer Test"]');
  expect(option).not.toBeNull();
  await act(async () => option!.click());
  await settle();
  expect(commands.at(-1)).toMatchObject({
    type: "switchProvider", sessionId: "host-session", expectedRevision: revision,
    harness: "cursor", model: cursor.id,
  });
  await vi.waitFor(() => expect(byLabel("Cursor Composer Test")).not.toBeNull(), { timeout: 4_000 });
  await send("Second");
  expect(commands.at(-1)).toMatchObject({ type: "send", sessionId: "host-session", text: "Second" });
  expect(commands.filter((command) => command.type === "create")).toHaveLength(1);
  expect(host?.session.blocks.map((block) => block.text)).toEqual(["First", "Done", "Second", "Done"]);
});

it("keeps started sessions on their provider when the host lacks switch support", async () => {
  providers = ["codex", "cursor"];
  catalog = { models: { codex: [gpt], cursor: [cursor] }, errors: {} };
  await render();
  await send("First");
  await vi.waitFor(() => expect(host?.session.blocks).toHaveLength(2));
  expect(await openProviderModels("Cursor")).toBeNull();
  expect(commands.some((command) => command.type === "switchProvider")).toBe(false);
});

function inspectionHost(id: string): HostSession {
  return {
    projectId: "project", revision: 9, status: "interrupted", updatedAt: 0,
    session: {
      id, cwd: "/home/me/repo", harness: "codex", model: gpt.id,
      modelSettings: {}, runtimeMode: "supervised", title: "Interrupted transfer",
      providerSessionId: "retained-native",
      blocks: [{ id: "submitted", role: "user", text: "This request may already have run" }],
      providerContext: {
        version: 1, bindings: [{ harness: "codex", cwd: "/home/me/repo", providerSessionId: "retained-native" }],
        delivery: { switchId: "switch", from: "claude", to: "codex", cwd: "/home/me/repo",
          currentUserBlockId: "submitted", includedBlockIds: [], omittedBlockIds: [],
          status: "uncertain", mode: "native", requestSubmitted: true, needsInspection: true },
      },
    },
  };
}

it("requires a revision-checked remote inspection confirmation without resubmitting the request", async () => {
  capabilities.push("sessionProviderInspectionV1", "sessionProviderSwitchV1");
  host = inspectionHost("inspection-current");
  rememberRemoteSession("shell", host.session.id);
  await render();
  expect(container.textContent).toContain("Inspect its work before continuing");
  expect(container.querySelector('[aria-label="Send remote draft"]')).toBeNull();
  await send("A future follow-up");
  expect(commands).toHaveLength(0);
  const confirm = [...container.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent === "Confirm inspection");
  expect(confirm).toBeDefined();
  await act(async () => confirm!.click());
  await settle();
  expect(commands).toHaveLength(1);
  expect(commands[0]).toMatchObject({ type: "confirmProviderInspection", sessionId: "inspection-current", expectedRevision: 9 });
  expect(host!.session.providerContext?.delivery).toBeUndefined();
  expect(host!.session.providerSessionId).toBe("retained-native");
  expect(host!.session.blocks).toHaveLength(1);
  expect(commands.some((command) => command.type === "send")).toBe(false);
});

it("keeps inspection commands unavailable on older hosts", async () => {
  capabilities.push("sessionProviderSwitchV1");
  host = inspectionHost("inspection-old-host");
  rememberRemoteSession("shell", host.session.id);
  await render();
  expect(container.textContent).toContain("Update this host to confirm inspection");
  expect([...container.querySelectorAll("button")].some((button) => button.textContent === "Confirm inspection")).toBe(false);
  await send("A future follow-up");
  expect(commands).toHaveLength(0);
});

it("retries a lost inspection receipt after remount with its original command ID", async () => {
  capabilities.push("sessionProviderInspectionV1");
  host = inspectionHost("inspection-lost-receipt");
  rememberRemoteSession("shell", host.session.id);
  const original = vi.mocked(invoke).getMockImplementation()!;
  let receipt: ReturnType<typeof dispatch> | undefined;
  const attempts: HostCommand[] = [];
  vi.mocked(invoke).mockImplementation(async (command, input) => {
    const request = input as { method?: string; params?: HostCommand } | undefined;
    if (request?.method === "commands.dispatch" && request.params?.type === "confirmProviderInspection") {
      attempts.push(request.params);
      if (receipt) return receipt;
      receipt = dispatch(request.params);
      throw new Error("Inspection receipt lost");
    }
    return original(command, input);
  });
  await render();
  const confirm = [...container.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent === "Confirm inspection")!;
  await act(async () => confirm.click());
  await settle();
  await act(async () => root.unmount());
  root = createRoot(container);
  await render();
  expect(attempts).toHaveLength(1);
  const retry = [...container.querySelectorAll<HTMLButtonElement>("button")].find((button) => button.textContent === "Retry")!;
  expect(retry).toBeDefined();
  await act(async () => retry.click());
  await settle();
  expect(attempts).toHaveLength(2);
  expect(attempts[1]).toEqual(attempts[0]);
  expect(commands.filter((command) => command.type === "confirmProviderInspection")).toHaveLength(1);
  expect(commands.some((command) => command.type === "send")).toBe(false);
});

it("waits for a running turn before applying the remote provider selection", async () => {
  capabilities.push("sessionProviderSwitchV1");
  providers = ["codex", "cursor"];
  catalog = { models: { codex: [gpt], cursor: [cursor] }, errors: {} };
  host = {
    projectId: "project", revision: 4, status: "running", runId: "active-run", updatedAt: 0,
    session: { id: "host-session", cwd: "/home/me/repo", harness: "codex", model: gpt.id, modelSettings: {}, runtimeMode: "supervised", title: "Existing", busy: true, blocks: [{ id: "first", role: "user", text: "First" }] },
  };
  rememberRemoteSession("shell", "host-session");
  await render();
  expect(byLabel("Stop")).not.toBeNull();
  await openProviderModels("Cursor");
  const option = document.body.querySelector<HTMLElement>('[role="option"][aria-label^="Composer Test"]')!;
  await act(async () => option.click());
  await settle();
  expect(commands.some((command) => command.type === "switchProvider" || command.type === "cancel")).toBe(false);
  host = { ...host!, revision: host!.revision + 1, status: "idle", runId: undefined, session: { ...host!.session, busy: false } };
  const revision = host.revision;
  await act(async () => { await new Promise((resolve) => setTimeout(resolve, 850)); });
  await settle();
  expect(commands.at(-1)).toMatchObject({ type: "switchProvider", harness: "cursor", expectedRevision: revision });
});

it("retries a lost provider selection receipt after remount with its original command ID", async () => {
  capabilities.push("sessionProviderSwitchV1");
  providers = ["codex", "cursor"];
  catalog = { models: { codex: [gpt], cursor: [cursor] }, errors: {} };
  const original = vi.mocked(invoke).getMockImplementation()!;
  let receipt: ReturnType<typeof dispatch> | undefined;
  const attempts: HostCommand[] = [];
  vi.mocked(invoke).mockImplementation(async (command, input) => {
    const request = input as { method?: string; params?: HostCommand } | undefined;
    if (request?.method === "commands.dispatch" && request.params?.type === "switchProvider") {
      attempts.push(request.params);
      if (receipt) return receipt;
      receipt = dispatch(request.params);
      throw new Error("Selection receipt lost");
    }
    return original(command, input);
  });
  await render();
  await send("First");
  await openProviderModels("Cursor");
  const option = document.body.querySelector<HTMLElement>('[role="option"][aria-label^="Composer Test"]')!;
  await act(async () => option.click());
  await settle();
  await act(async () => root.unmount());
  root = createRoot(container);
  await render();
  const retry = [...container.querySelectorAll("button")].find((button) => button.textContent === "Retry")!;
  expect(retry).toBeTruthy();
  await act(async () => retry.click());
  await settle();
  expect(attempts).toHaveLength(2);
  expect(attempts[1]).toEqual(attempts[0]);
  expect(commands.filter((command) => command.type === "switchProvider")).toHaveLength(1);
});

it("keeps a saved model's effort editable when the host catalog fails", async () => {
  await render();
  await send("First");
  catalog = new Error("Codex CLI is not authenticated");
  await act(async () => root.unmount());
  root = createRoot(container);
  await render();
  expect(container.textContent).toContain("Couldn’t load models");
  expect(byLabel("Reasoning:")?.getAttribute("aria-label")).toBe(
    "Reasoning: High",
  );
  await chooseEffort("Low");
  expect(commands.at(-1)).toMatchObject({
    type: "configure",
    model: "codex:gpt-test",
    modelSettings: { reasoningEffort: "low" },
  });
});

it("asks to connect the machine when it is not set up on this computer", async () => {
  vi.mocked(invoke).mockImplementation(async (command) =>
    command === "remote_machines" ? [] : undefined,
  );
  await render();
  expect(container.textContent).toContain(
    "The machine for this project isn’t connected on this computer.",
  );
  expect(container.querySelector("textarea")).toBeNull();
});

it("holds a settings change during a running turn and applies it afterwards", async () => {
  await render();
  await send("First");
  host = {
    ...host!,
    revision: host!.revision + 1,
    status: "running",
    runId: "run",
    session: { ...host!.session, busy: true },
  };
  await vi.waitFor(() => expect(byLabel("Stop")).not.toBeNull(), {
    timeout: 4_000,
  });
  await chooseEffort("Low");
  expect(commands.some((command) => command.type === "configure")).toBe(false);

  await act(async () => byLabel("Stop")!.click());
  expect(commands.at(-1)).toMatchObject({ type: "cancel", runId: "run" });

  host = {
    ...host!,
    revision: host!.revision + 1,
    status: "idle",
    runId: undefined,
    session: { ...host!.session, busy: false },
  };
  await vi.waitFor(
    () =>
      expect(commands.at(-1)).toMatchObject({
        type: "configure",
        modelSettings: { reasoningEffort: "low" },
      }),
    { timeout: 4_000 },
  );
});

it.each([false, true])("retries a lost create response without duplicating the first turn (remount: %s)", async (remount) => {
  const original = vi.mocked(invoke).getMockImplementation()!;
  let accepted: ReturnType<typeof dispatch> | undefined;
  const attempts: HostCommand[] = [];
  vi.mocked(invoke).mockImplementation(async (command, input) => {
    const request = input as { method?: string; params?: HostCommand } | undefined;
    if (request?.method === "commands.dispatch" && request.params?.type === "create") {
      attempts.push(request.params);
      if (accepted) return accepted;
      accepted = dispatch(request.params);
      throw new Error("Response lost after host accepted the request");
    }
    return original(command, input);
  });
  await render();
  await send("Keep this first message");
  expect(commands.map((command) => command.type)).toEqual(["create"]);
  if (remount) {
    await act(async () => root.unmount());
    root = createRoot(container);
    await render();
  }
  const retry = [...container.querySelectorAll("button")].find((button) => button.textContent === "Retry")!;
  expect(retry).toBeTruthy();
  await act(async () => retry.click());
  await settle();
  expect(attempts).toHaveLength(2);
  expect(attempts[1]).toEqual(attempts[0]);
  expect(commands.map((command) => command.type)).toEqual(["create", "send"]);
  expect(commands[1]).toMatchObject({ text: "Keep this first message", sessionId: "host-session" });
  expect(transcriptItems("Keep this first message")).toHaveLength(1);
});

it("ignores a late create response after its tab has switched conversations", async () => {
  await render();
  let release!: () => void;
  dispatchDelay = new Promise<void>((resolve) => { release = resolve; });
  await send("Pending first message");
  await act(async () => rememberRemoteSession("shell", "different-session"));
  await act(async () => { release(); dispatchDelay = undefined; });
  await settle();
  expect(remoteSessionFor("shell")).toBe("different-session");
  expect(commands.map((command) => command.type)).toEqual(["create"]);
  expect(container.textContent).not.toContain("Pending first message");
});
