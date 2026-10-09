import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import type { SessionPaneProps } from "../../sessions/ui/SessionPane";
import type {
  Attachment,
  Block,
  ComposerTurnOptions,
  HarnessId,
  RuntimeMode,
  Session,
  WorkspaceMode,
  PlanBuildTarget,
} from "../../sessions/model/session";
import { uploadRemoteAttachments } from "../model/remoteAttachments";
import { temporaryWorktreeBranchName } from "../../source-control/model/worktrees";
import type { AgentModel } from "../../sessions/model/models";
import {
  ModelSourceContext,
  type ModelSource,
} from "../../sessions/ui/modelSource";
import { notifyGitChanged } from "../../../platform/tauri/fs";
import type { Worktree } from "../../source-control/model/worktrees";
import { useProjectBranchesState } from "../../source-control/hooks/useProjectBranches";
import { registerRemoteSessionActions } from "../model/remoteSessionActions";
import {
  clearPendingRemoteCommand,
  loadRemoteSession,
  OPEN_CONNECTIONS_EVENT,
  pendingRemoteCommand,
  pendingRemoteFollowup,
  rememberRemotePendingWorktree,
  rememberRemoteSession,
  REMOTE_HISTORY_CHANGE,
  remoteRequest,
  reportRemoteMachineStatus,
  remotePendingWorktree,
  remoteSessionFor,
  savePendingRemoteCommand,
  useRemoteMachines,
} from "../model/connections";
import {
  parseRemotePath,
  remotePath,
  remoteProjectFor,
  type RemoteProject,
} from "../model/remoteProjects";
import {
  carryModelSettings,
  findRemoteModel,
  remoteModelControls,
  sameModelSettings,
} from "../model/remoteModels";
import {
  isRemoteProvider,
  REMOTE_PROVIDERS,
  requireHostDescriptor,
  type CommandReceipt,
  type HostCommand,
  type HostDescriptor,
  type HostModelCatalog,
  type HostSession,
  type HostWorktree,
  type RemoteAttachment,
  type RemoteMachine,
  type RemoteProvider,
} from "../model/protocol";

export type RemoteSessionOverrides = Partial<SessionPaneProps> & {
  remoteSession: boolean;
  remoteFeatures: { attachments: boolean; plan: boolean; draft: boolean };
  remoteSessionLoading: boolean;
  remoteSessionStarted: boolean;
  allowedModelHarnesses: readonly HarnessId[];
};

type Configuration = {
  harness: RemoteProvider;
  model: string;
  settings: Record<string, string>;
  mode: RuntimeMode;
};

type OptimisticTurn = {
  commandId: string;
  text: string;
  attachments: Attachment[];
  intent: "default" | "plan" | "build";
  draft?: boolean;
  draftBlockId?: string;
  planBlockId?: string;
  startedAt: number;
  turnModel: NonNullable<Block["turnModel"]>;
};

const noop = () => {};
const cachedSessionSnapshots = new Map<string, HostSession>();
const cachedDescriptors = new Map<string, HostDescriptor>();
const cachedCatalogs = new Map<string, HostModelCatalog>();
const snapshotKey = (machineId: string, sessionId: string) =>
  `${machineId}:${sessionId}`;
const catalogKey = (machineId: string, projectId: string) =>
  JSON.stringify([machineId, projectId]);
function rememberSessionSnapshot(key: string, snapshot: HostSession) {
  cachedSessionSnapshots.delete(key);
  cachedSessionSnapshots.set(key, snapshot);
  if (cachedSessionSnapshots.size > 8)
    cachedSessionSnapshots.delete(cachedSessionSnapshots.keys().next().value!);
}

/** Fetches a host conversation into the snapshot cache, so its tab opens with
 * the transcript already laid out, as a local session read from disk does. */
export async function preloadRemoteSession(
  machineId: string,
  sessionId: string,
): Promise<void> {
  const key = snapshotKey(machineId, sessionId);
  if (cachedSessionSnapshots.has(key)) return;
  rememberSessionSnapshot(key, await loadRemoteSession(machineId, sessionId));
}

/** A tab in a project on another machine. The host owns the session; this
 * renders the normal session pane with actions routed to the host. */
export function RemoteSession({
  shell,
  visible,
  onSnapshot,
  onOpenFile,
  onOpenDiff,
  onOpenPlan,
  render,
}: {
  /** The tab's local session, which provides its ID and new-session defaults. */
  shell: Session;
  visible: boolean;
  onSnapshot?: (shellId: string, snapshot?: HostSession) => void;
  onOpenFile: SessionPaneProps["onOpenFile"];
  onOpenDiff: SessionPaneProps["onOpenDiff"];
  onOpenPlan: SessionPaneProps["onOpenPlan"];
  render: (overrides: RemoteSessionOverrides) => ReactNode;
}) {
  const project = remoteProjectFor(shell.cwd);
  const { machines, loaded } = useRemoteMachines(!!project);
  const machine = project
    ? machines.find((entry) => entry.environmentId === project.environmentId)
    : undefined;
  if (!project || !machine)
    return (
      <div className="flex h-full flex-col items-center justify-center gap-3 p-6 text-center">
        <p className="text-[13px] text-content/60">
          {!project
            ? "This project’s machine details are missing. Add the project again from the project rail."
            : loaded
              ? "The machine for this project isn’t connected on this computer."
              : "Connecting to the machine…"}
        </p>
        {project && loaded ? (
          <button
            type="button"
            className="rounded-lg bg-selection px-3 py-1.5 text-[12px] hover:bg-selection-hover"
            onClick={() =>
              window.dispatchEvent(new Event(OPEN_CONNECTIONS_EVENT))
            }
          >
            Manage machines
          </button>
        ) : null}
      </div>
    );
  return (
    <ConnectedRemoteSession
      key={`${machine.id}:${shell.id}`}
      shell={shell}
      visible={visible}
      onSnapshot={onSnapshot}
      onOpenFile={onOpenFile}
      onOpenDiff={onOpenDiff}
      onOpenPlan={onOpenPlan}
      machine={machine}
      project={project}
      render={render}
    />
  );
}

function ConnectedRemoteSession({
  shell,
  visible,
  onSnapshot,
  onOpenFile,
  onOpenDiff,
  onOpenPlan,
  machine,
  project,
  render,
}: {
  shell: Session;
  visible: boolean;
  onSnapshot?: (shellId: string, snapshot?: HostSession) => void;
  onOpenFile: SessionPaneProps["onOpenFile"];
  onOpenDiff: SessionPaneProps["onOpenDiff"];
  onOpenPlan: SessionPaneProps["onOpenPlan"];
  machine: RemoteMachine;
  project: RemoteProject;
  render: (overrides: RemoteSessionOverrides) => ReactNode;
}) {
  const [descriptor, setDescriptor] = useState<HostDescriptor | undefined>(() =>
    cachedDescriptors.get(machine.id),
  );
  const [online, setOnline] = useState(false);
  const [error, setError] = useState("");
  const [sessionId, setSessionId] = useState(() => remoteSessionFor(shell.id));
  const boundSession = useRef(sessionId);
  const bindingVersion = useRef(0);
  const deletingSession = useRef<string | undefined>(undefined);
  useEffect(() => {
    const changed = () => {
      const next = remoteSessionFor(shell.id);
      if (next !== boundSession.current) {
        bindingVersion.current++;
        boundSession.current = next;
        setStarting(undefined);
        setUnseenSend(undefined);
        setChanges(undefined);
        applied.current = undefined;
        setError("");
        setRemovingDraft(undefined);
        preparingRef.current = false;
        setSnapshot(
          next
            ? cachedSessionSnapshots.get(snapshotKey(machine.id, next))
            : undefined,
        );
      }
      setPending(
        pendingRemoteCommand(
          project.key,
          machine.environmentId,
          next ?? null,
          shell.id,
        ),
      );
      setSessionId(next);
    };
    window.addEventListener(REMOTE_HISTORY_CHANGE, changed);
    changed();
    return () => window.removeEventListener(REMOTE_HISTORY_CHANGE, changed);
  }, [shell.id, machine.id]);
  const [snapshot, setSnapshot] = useState<HostSession | undefined>(() =>
    sessionId
      ? cachedSessionSnapshots.get(snapshotKey(machine.id, sessionId))
      : undefined,
  );
  const snapshotRef = useRef(snapshot);
  snapshotRef.current = snapshot;
  const [refresh, setRefresh] = useState(0);
  const [catalog, setCatalog] = useState<HostModelCatalog | undefined>(() =>
    cachedCatalogs.get(catalogKey(machine.id, project.projectId)),
  );
  const [catalogError, setCatalogError] = useState("");
  const [catalogRefresh, setCatalogRefresh] = useState(0);
  const [selectedCwd, setSelectedCwd] = useState(
    () => remotePendingWorktree(shell.id) ?? project.cwd,
  );
  const [draftWorkspaceMode, setDraftWorkspaceMode] =
    useState<WorkspaceMode>("current");
  const [draftWorktreeBase, setDraftWorktreeBase] = useState("HEAD");
  const [sending, setSending] = useState(false);
  const sendingRef = useRef(false);
  const preparingRef = useRef(false);
  const [unseenSend, setUnseenSend] = useState<
    Pick<
      OptimisticTurn,
      | "commandId"
      | "text"
      | "startedAt"
      | "turnModel"
      | "attachments"
      | "draftBlockId"
    > & {
      sessionId: string;
    }
  >();
  // A first message waits here while its session is created on the host.
  const [starting, setStarting] = useState<
    OptimisticTurn & { failed?: boolean }
  >();
  const [pending, setPending] = useState(() =>
    pendingRemoteCommand(
      project.key,
      machine.environmentId,
      sessionId ?? null,
      shell.id,
    ),
  );
  const [draft, setDraft] = useState<Configuration>(() => ({
    harness: isRemoteProvider(shell.harness) ? shell.harness : "codex",
    model: shell.model,
    settings: shell.modelSettings ?? {},
    mode: shell.runtimeMode,
  }));
  // Changes to a started session, applied when it is idle.
  const [changes, setChanges] = useState<Configuration>();
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);
  useEffect(() => {
    setPending(
      pendingRemoteCommand(
        project.key,
        machine.environmentId,
        sessionId ?? null,
        shell.id,
      ),
    );
  }, [project.key, machine.environmentId, sessionId]);

  const hostSession =
    snapshot && snapshot.session.id === sessionId
      ? snapshot.session
      : undefined;
  useEffect(() => {
    if (hostSession) rememberRemotePendingWorktree(shell.id);
  }, [hostSession?.id, shell.id]);
  const executionCwd = hostSession?.cwd ?? selectedCwd;
  const { branches } = useProjectBranchesState(
    remotePath(machine.environmentId, executionCwd),
    online,
  );
  const activeSessionId = hostSession?.id ?? sessionId;
  const hasHostBlock = (commandId: string) =>
    !!hostSession?.blocks.some((block) => block.id === commandId);
  const unseenActive =
    !!unseenSend &&
    unseenSend.sessionId === activeSessionId &&
    !hasHostBlock(unseenSend.commandId);
  const startingActive =
    !!starting && !starting.failed && !hasHostBlock(starting.commandId);
  const pendingSendActive =
    (pending?.type === "send" || pending?.type === "compact") &&
    pending.sessionId === activeSessionId &&
    !hasHostBlock(pending.commandId);
  const busy =
    !!hostSession?.busy ||
    unseenActive ||
    (startingActive && !starting?.draft) ||
    pendingSendActive;
  // An accepted turn stays on screen until a sync shows the host's copy, so
  // the transcript never drops it for a moment in between.
  useEffect(() => {
    if (starting && !starting.failed && hasHostBlock(starting.commandId))
      setStarting(undefined);
  }, [hostSession, starting]);
  // A draft being removed leaves the transcript at once, as it does locally,
  // and returns if the host turns the removal down.
  const [removingDraft, setRemovingDraft] = useState<string>();
  useEffect(() => {
    if (
      removingDraft &&
      !hostSession?.blocks.some((block) => block.id === removingDraft)
    )
      setRemovingDraft(undefined);
  }, [hostSession, removingDraft]);
  useEffect(() => {
    if (
      unseenSend &&
      hostSession?.id === unseenSend.sessionId &&
      hostSession.blocks.some((block) => block.id === unseenSend.commandId)
    )
      setUnseenSend((current) =>
        current?.commandId === unseenSend.commandId ? undefined : current,
      );
  }, [hostSession, unseenSend]);

  useEffect(() => {
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    let failed = 0;
    const version = bindingVersion.current;
    const stale = () =>
      disposed ||
      version !== bindingVersion.current ||
      (!!sessionId && deletingSession.current === sessionId);
    // Every request carries the expected host identity; describe again only
    // after a failure, when the host may have been replaced.
    let described = false;
    const poll = async () => {
      let active = false;
      try {
        if (!described) {
          const host = requireHostDescriptor(
            await remoteRequest<HostDescriptor>(
              machine.id,
              "environment.describe",
              { supportedProviders: REMOTE_PROVIDERS },
            ),
          );
          if (host.environmentId !== machine.environmentId)
            throw new Error(
              "Host identity changed. Reconnect this machine before continuing.",
            );
          if (stale()) return;
          cachedDescriptors.set(machine.id, host);
          setDescriptor(host);
          described = true;
        }
        const known =
          snapshotRef.current?.session.id === sessionId
            ? snapshotRef.current
            : undefined;
        const next = sessionId
          ? await loadRemoteSession(machine.id, sessionId, known)
          : undefined;
        if (stale()) return;
        if (next && next.projectId !== project.projectId)
          throw new Error("This session belongs to a different host project");
        setOnline(true);
        reportRemoteMachineStatus(machine.id, true);
        // A catalog request that failed while offline is retried on recovery.
        if (failed) setCatalogRefresh((value) => value + 1);
        failed = 0;
        if (next && sessionId)
          rememberSessionSnapshot(snapshotKey(machine.id, sessionId), next);
        setSnapshot(next);
        if (next) onSnapshot?.(shell.id, next);
        active = !!next?.session.busy;
      } catch (reason) {
        if (stale()) return;
        setOnline(false);
        reportRemoteMachineStatus(machine.id, false);
        described = false;
        failed++;
      }
      if (!disposed)
        timer = setTimeout(
          () => void poll(),
          failed
            ? Math.min(10_000, 750 * 2 ** Math.min(failed, 4))
            : active
              ? 750
              : visible
                ? 3_000
                : 10_000,
        );
    };
    void poll();
    return () => {
      disposed = true;
      clearTimeout(timer);
    };
  }, [
    machine.id,
    machine.environmentId,
    project.projectId,
    sessionId,
    refresh,
    visible,
    onSnapshot,
  ]);

  useEffect(() => {
    if (!descriptor) return;
    let disposed = false;
    void remoteRequest<HostModelCatalog>(machine.id, "models.list", {
      projectId: project.projectId,
    })
      .then((value) => {
        if (disposed) return;
        cachedCatalogs.set(catalogKey(machine.id, project.projectId), value);
        setCatalog(value);
        setCatalogError("");
      })
      .catch((reason) => {
        if (!disposed) setCatalogError(String(reason));
      });
    return () => {
      disposed = true;
    };
  }, [
    machine.id,
    descriptor?.environmentId,
    project.projectId,
    catalogRefresh,
  ]);

  const providers = useMemo(
    () => (descriptor?.providers ?? []).filter(isRemoteProvider),
    [descriptor],
  );
  // A new session starts with the tab's model when the host offers it, and
  // otherwise with the host's first model. Settings follow the host's entry:
  // the same id can differ between machines, such as `claude:opus` offering a
  // 1M context only on an account that has it.
  useEffect(() => {
    if (!online || sessionId || !catalog || !providers.length) return;
    const harness = providers.includes(draft.harness)
      ? draft.harness
      : providers[0];
    const models = catalog.models[harness] ?? [];
    const model =
      findRemoteModel(models, draft.model) ??
      (harness === draft.harness ? undefined : models[0]) ??
      models[0];
    if (!model) return;
    setDraft((current) => {
      const settings = carryModelSettings(
        model.settings ?? [],
        current.settings,
      );
      return current.harness === harness &&
        current.model === model.id &&
        sameModelSettings(settings, current.settings)
        ? current
        : { ...current, harness, model: model.id, settings };
    });
  }, [online, catalog, providers, sessionId, draft.harness, draft.model]);

  const saved: Configuration | undefined = hostSession && {
    harness: hostSession.harness as RemoteProvider,
    model: hostSession.model,
    settings: hostSession.modelSettings ?? {},
    mode: hostSession.runtimeMode,
  };
  const configuration = saved ? (changes ?? saved) : draft;
  const updateConfiguration = (
    update: (current: Configuration) => Configuration,
  ) => {
    if (saved) setChanges(update(changes ?? saved));
    else setDraft(update);
  };

  const selectedTurnModel = (): NonNullable<Block["turnModel"]> => ({
    harness: configuration.harness,
    id: configuration.model,
    name:
      findRemoteModel(
        catalog?.models[configuration.harness] ?? [],
        configuration.model,
      )?.name ?? configuration.model.replace(/^[^:]+:/, ""),
  });
  const optimisticTurn = (
    text: string,
    attachments: Attachment[] = [],
    intent: "default" | "plan" | "build" = "default",
    draft = false,
    draftBlockId?: string,
    planBlockId?: string,
  ): OptimisticTurn => ({
    commandId: crypto.randomUUID(),
    text,
    attachments,
    intent,
    draft,
    draftBlockId,
    planBlockId,
    startedAt: Date.now(),
    turnModel: selectedTurnModel(),
  });

  const run = async (
    command: HostCommand,
    optimistic?: OptimisticTurn,
    followup?: HostCommand,
  ): Promise<CommandReceipt | undefined> => {
    if (sendingRef.current) return undefined;
    const version = bindingVersion.current;
    sendingRef.current = true;
    setSending(true);
    setError("");
    if (command.type === "send" || command.type === "compact")
      setUnseenSend((current) =>
        current?.commandId === command.commandId
          ? current
          : {
              sessionId: command.sessionId,
              commandId: command.commandId,
              text: command.type === "send" ? command.text : "/compact",
              attachments: optimistic?.attachments ?? [],
              startedAt: optimistic?.startedAt ?? Date.now(),
              turnModel: optimistic?.turnModel ?? selectedTurnModel(),
              draftBlockId:
                command.type === "send" ? command.draftBlockId : undefined,
            },
      );
    // Keep the original ID across disconnects and app restarts. An ambiguous
    // response is retried explicitly instead of silently sending a new prompt.
    try {
      savePendingRemoteCommand(
        project.key,
        machine.environmentId,
        command,
        shell.id,
        followup,
      );
      setPending(command);
      const receipt = await remoteRequest<CommandReceipt>(
        machine.id,
        "commands.dispatch",
        command,
      );
      if (command.type === "create") {
        const next = pendingRemoteFollowup(
          project.key,
          machine.environmentId,
          command.commandId,
        );
        if (next && next.type !== "create")
          savePendingRemoteCommand(
            project.key,
            machine.environmentId,
            { ...next, sessionId: receipt.sessionId },
            shell.id,
          );
        if (version === bindingVersion.current) openSession(receipt.sessionId);
      }
      clearPendingRemoteCommand(
        project.key,
        machine.environmentId,
        command.commandId,
      );
      if (!alive.current || version !== bindingVersion.current) return receipt;
      if (command.type === "draft") {
        // Accepted drafts are actionable before the next snapshot arrives.
        setSnapshot((current) => ({
          ...(current?.session.id === command.sessionId
            ? current
            : {
                projectId: project.projectId,
                revision: 0,
                updatedAt: Date.now(),
              }),
          status: "idle",
          session: {
            ...(current?.session.id === command.sessionId
              ? current.session
              : shell),
            id: command.sessionId,
            cwd: selectedCwd,
            harness: configuration.harness,
            model: configuration.model,
            modelSettings: configuration.settings,
            runtimeMode: configuration.mode,
            busy: false,
            blocks: [
              ...(current?.session.id === command.sessionId
                ? current.session.blocks
                : []
              ).filter(
                (block) => !block.draft && block.id !== command.commandId,
              ),
              {
                id: command.commandId,
                role: "user",
                text: command.text,
                draft: true,
                attachments: optimistic?.attachments ?? [],
              },
            ],
          },
        }));
        setStarting(undefined);
      }
      setPending(
        pendingRemoteCommand(
          project.key,
          machine.environmentId,
          command.type === "create" ? receipt.sessionId : (sessionId ?? null),
          shell.id,
        ),
      );
      setRefresh((value) => value + 1);
      return receipt;
    } catch (reason) {
      if (!alive.current || version !== bindingVersion.current)
        return undefined;
      const message = String(reason);
      if (message.includes("Host rejected request:")) {
        if (command.type === "send" || command.type === "compact")
          setUnseenSend((current) =>
            current?.commandId === command.commandId ? undefined : current,
          );
        clearPendingRemoteCommand(
          project.key,
          machine.environmentId,
          command.commandId,
        );
        setPending(
          pendingRemoteCommand(
            project.key,
            machine.environmentId,
            sessionId ?? null,
            shell.id,
          ),
        );
      }
      setError(message.replace(/^Error: /, ""));
      return undefined;
    } finally {
      sendingRef.current = false;
      if (alive.current) setSending(false);
    }
  };

  // A conversation that was only a draft goes with it, as a local one does,
  // and the tab starts over as a new conversation.
  const discardSession = async (id: string) => {
    const version = bindingVersion.current;
    deletingSession.current = id;
    try {
      await remoteRequest(machine.id, "sessions.delete", {
        projectId: project.projectId,
        sessionId: id,
      });
      cachedSessionSnapshots.delete(snapshotKey(machine.id, id));
      if (!alive.current || version !== bindingVersion.current) return;
      setSnapshot(undefined);
      rememberRemoteSession(shell.id);
      onSnapshot?.(shell.id, undefined);
    } catch (reason) {
      if (!alive.current || version !== bindingVersion.current) return;
      deletingSession.current = undefined;
      setRefresh((value) => value + 1);
      setRemovingDraft(undefined);
      setError(String(reason).replace(/^Error: /, ""));
    }
  };

  const openSession = (id: string) => {
    boundSession.current = id;
    rememberRemoteSession(shell.id, id);
    setSessionId(id);
  };

  // Model, effort and permission changes apply directly, as locally. A
  // running turn keeps its settings; the change is sent once it finishes.
  const applying = useRef(false);
  // The last change the host accepted, until a sync reflects it.
  const applied = useRef<Configuration>(undefined);
  useEffect(() => {
    if (!changes || !saved || !hostSession) return;
    const same = (a: Configuration, b: Configuration) =>
      a.model === b.model &&
      a.mode === b.mode &&
      sameModelSettings(a.settings, b.settings);
    if (same(changes, saved)) {
      applied.current = undefined;
      setChanges(undefined);
      return;
    }
    if (applied.current && same(changes, applied.current)) return;
    if (busy || !online || pending || applying.current) return;
    applying.current = true;
    const sent = changes;
    void run({
      type: "configure",
      commandId: crypto.randomUUID(),
      sessionId: hostSession.id,
      model: changes.model,
      modelSettings: changes.settings,
      runtimeMode: changes.mode,
    })
      .then((receipt) => {
        if (receipt) applied.current = sent;
      })
      .finally(() => {
        applying.current = false;
      });
  });

  const dispatchTurn = async (
    id: string,
    turn: OptimisticTurn,
    uploaded?: RemoteAttachment[],
  ) => {
    const version = bindingVersion.current;
    const refs = turn.draftBlockId
      ? []
      : (uploaded ??
        (await uploadRemoteAttachments(machine.id, turn.attachments)));
    if (!alive.current || version !== bindingVersion.current) return undefined;
    return run(
      turn.draft
        ? {
            type: "draft",
            commandId: turn.commandId,
            sessionId: id,
            text: turn.text,
            attachments: refs,
          }
        : message(
            id,
            turn.text,
            turn.commandId,
            refs,
            turn.intent,
            turn.draftBlockId,
            turn.planBlockId,
          ),
      turn,
    );
  };

  const startSession = async (turn: OptimisticTurn) => {
    const version = bindingVersion.current;
    try {
      const uploaded = await uploadRemoteAttachments(
        machine.id,
        turn.attachments,
      );
      if (version !== bindingVersion.current) return;
      let worktreeCwd = selectedCwd;
      let autoWorktreeBranch: string | undefined;
      if (draftWorkspaceMode === "worktree") {
        try {
          const tree = await remoteRequest<HostWorktree>(
            machine.id,
            "git.worktreeCreate",
            {
              projectId: project.projectId,
              cwd: selectedCwd,
              branch: temporaryWorktreeBranchName(),
              base: draftWorktreeBase,
              existing: false,
            },
          );
          if (version !== bindingVersion.current) return;
          worktreeCwd = tree.path;
          autoWorktreeBranch = tree.branch ?? undefined;
          rememberRemotePendingWorktree(shell.id, tree.path);
          if (alive.current) {
            setSelectedCwd(tree.path);
            setDraftWorkspaceMode("current");
          }
        } catch (reason) {
          if (alive.current && version === bindingVersion.current) {
            setError(String(reason));
            setStarting({ ...turn, failed: true });
          }
          return;
        }
      }
      const followup: Exclude<HostCommand, { type: "create" }> = turn.draft
        ? {
            type: "draft",
            commandId: turn.commandId,
            sessionId: "",
            text: turn.text,
            attachments: uploaded,
          }
        : message(
            "",
            turn.text,
            turn.commandId,
            uploaded,
            turn.intent,
            turn.draftBlockId,
            turn.planBlockId,
          );
      const receipt = await run(
        {
          type: "create",
          commandId: crypto.randomUUID(),
          projectId: project.projectId,
          ...(worktreeCwd !== project.cwd ? { worktreeCwd } : {}),
          ...(autoWorktreeBranch ? { autoWorktreeBranch } : {}),
          harness: draft.harness,
          model: draft.model,
          modelSettings: draft.settings,
          runtimeMode: draft.mode,
        },
        turn,
        followup,
      );
      if (version !== bindingVersion.current) return;
      if (!receipt) {
        if (alive.current) setStarting({ ...turn, failed: true });
        return;
      }
      if (version !== bindingVersion.current) return;
      const sent = await run(
        { ...followup, sessionId: receipt.sessionId },
        turn,
      );
      if (alive.current && version === bindingVersion.current)
        if (!sent) setStarting({ ...turn, failed: true });
    } catch (reason) {
      if (alive.current && version === bindingVersion.current) {
        setError(String(reason));
        setStarting({ ...turn, failed: true });
      }
    } finally {
      preparingRef.current = false;
    }
  };

  const message = (
    id: string,
    text: string,
    commandId: string = crypto.randomUUID(),
    attachments: RemoteAttachment[] = [],
    intent: "default" | "plan" | "build" = "default",
    draftBlockId?: string,
    planBlockId?: string,
  ): Extract<HostCommand, { type: "send" | "compact" }> =>
    text.trim().toLowerCase() === "/compact" &&
    !attachments.length &&
    !draftBlockId &&
    intent === "default"
      ? { type: "compact", commandId, sessionId: id }
      : {
          type: "send",
          commandId,
          sessionId: id,
          text,
          attachments,
          intent,
          ...(draftBlockId ? { draftBlockId } : {}),
          ...(planBlockId ? { planBlockId } : {}),
        };

  const submit = (
    text: string,
    attachments: Attachment[] = [],
    options?: ComposerTurnOptions,
    asDraft = false,
    planBlockId?: string,
    onReceipt?: (accepted: boolean) => void,
  ): boolean => {
    if (
      !online ||
      sending ||
      preparingRef.current ||
      pending ||
      busy ||
      (!text.trim() && !attachments.length)
    )
      return false;
    const intent =
      options?.intent === "plan" || options?.intent === "build"
        ? options.intent
        : "default";
    const turn = optimisticTurn(
      text,
      attachments,
      intent,
      asDraft,
      options?.draftBlockId,
      planBlockId,
    );
    if (!hostSession && onReceipt) return false;
    preparingRef.current = true;
    setStarting(turn);
    if (!hostSession) {
      if (sessionId || !draft.model) {
        preparingRef.current = false;
        setStarting(undefined);
        return false;
      }
      void startSession(turn);
      return true;
    }
    if (changes) {
      preparingRef.current = false;
      setStarting(undefined);
      return false;
    }
    const version = bindingVersion.current;
    void dispatchTurn(hostSession.id, turn)
      .then((receipt) => {
        onReceipt?.(receipt !== undefined);
        if (alive.current && version === bindingVersion.current)
          if (!receipt) setStarting({ ...turn, failed: true });
      })
      .catch((reason) => {
        onReceipt?.(false);
        if (alive.current && version === bindingVersion.current) {
          setError(String(reason));
          setStarting({ ...turn, failed: true });
        }
      })
      .finally(() => {
        preparingRef.current = false;
      });
    return true;
  };

  const modelSource = useMemo<ModelSource>(() => {
    const models = (harness: HarnessId) =>
      catalog?.models[harness as RemoteProvider] ?? [];
    const savedModel = hostSession?.model;
    const savedSettings = hostSession?.modelSettings ?? {};
    return {
      id: `remote:${machine.environmentId}`,
      modelsFor: models,
      resolve: (harness, id = "") => {
        const provider = harness as RemoteProvider;
        // Keep the saved model's effort visible even when the host catalog is
        // loading, failed, or no longer lists it.
        const controls = remoteModelControls(
          catalog,
          provider,
          id,
          id === savedModel ? savedSettings : {},
          savedModel,
        );
        const listed = controls.model;
        return {
          ...(listed ?? {
            id,
            harness,
            name: id ? id.replace(/^[a-z]+:/, "") : "Loading models…",
            nativeId: id.replace(/^[a-z]+:/, ""),
          }),
          settings: controls.settings,
        } satisfies AgentModel;
      },
      find: (id) =>
        providers
          .flatMap((harness) => models(harness))
          .find((m) => m.id === id),
      available: (harness) =>
        providers.includes(harness as RemoteProvider) &&
        (!hostSession || hostSession.harness === harness),
      probed: () => !!descriptor,
      // The host re-probes when a provider CLI changes or its catalog ages,
      // so each picker opening asks again.
      refresh: () => setCatalogRefresh((value) => value + 1),
    };
  }, [
    catalog,
    catalogError,
    descriptor,
    providers,
    machine.environmentId,
    hostSession?.harness,
    hostSession?.model,
    hostSession?.modelSettings,
  ]);

  // Show a message the host has not confirmed yet in the transcript.
  // A draft being sent is replaced by its message at once, as locally.
  const leavingDrafts = new Set(
    [
      removingDraft,
      startingActive ? starting?.draftBlockId : undefined,
      pendingSendActive && pending?.type === "send"
        ? pending.draftBlockId
        : undefined,
      unseenActive ? unseenSend?.draftBlockId : undefined,
    ].filter(Boolean),
  );
  const blocks: Block[] = (hostSession?.blocks ?? []).filter(
    (block) => !leavingDrafts.has(block.id),
  );
  const unconfirmed: Block | undefined =
    unseenActive && unseenSend
      ? {
          id: unseenSend.commandId,
          role: "user",
          text: unseenSend.text,
          attachments: unseenSend.attachments,
          startedAt: unseenSend.startedAt,
          turnModel: unseenSend.turnModel,
        }
      : pendingSendActive && pending
        ? {
            id: pending.commandId,
            role: "user",
            text: pending.type === "send" ? pending.text : "/compact",
          }
        : startingActive && starting
          ? {
              id: starting.commandId,
              role: "user",
              text: starting.text,
              attachments: starting.attachments,
              draft: starting.draft,
              startedAt: starting.startedAt,
              turnModel: starting.turnModel,
            }
          : undefined;
  const session: Session = {
    ...(hostSession ?? {
      title: shell.title,
      blocks: [],
    }),
    id: shell.id,
    cwd: remotePath(machine.environmentId, project.cwd),
    worktreeCwd:
      executionCwd === project.cwd
        ? undefined
        : remotePath(machine.environmentId, executionCwd),
    workspaceMode: hostSession ? undefined : draftWorkspaceMode,
    worktreeBase: hostSession ? undefined : draftWorktreeBase,
    branch: branches?.current ?? hostSession?.branch,
    harness: configuration.harness,
    model: configuration.model,
    modelSettings: configuration.settings,
    runtimeMode: configuration.mode,
    busy,
    blocks: unconfirmed ? [...blocks, unconfirmed] : blocks,
  };

  const catalogProblem = catalog?.errors[configuration.harness] ?? catalogError;
  const retryPending = async () => {
    if (!pending || sendingRef.current) return;
    const version = bindingVersion.current;
    setStarting((current) =>
      current ? { ...current, failed: false } : current,
    );
    const receipt = await run(pending);
    if (
      receipt &&
      pending.type === "create" &&
      version === bindingVersion.current
    ) {
      const next = pendingRemoteCommand(
        project.key,
        machine.environmentId,
        receipt.sessionId,
        shell.id,
      );
      if (next) await run(next);
    }
  };
  const notice =
    pending && !sending
      ? {
          text: "Waiting for the host to confirm your request.",
          detail: error,
          action: { label: "Retry", run: () => void retryPending() },
        }
      : starting?.failed
        ? {
            text: `Couldn’t ${starting.draft ? "save the draft" : "send the message"} on ${machine.name}.`,
            detail: error,
            action: {
              label: "Try again",
              run: () => {
                const turn = { ...starting, failed: false };
                setStarting(turn);
                preparingRef.current = true;
                if (!sessionId) void startSession(turn);
                else
                  void dispatchTurn(sessionId, turn)
                    .then((sent) => {
                      if (alive.current)
                        if (!sent) setStarting({ ...turn, failed: true });
                    })
                    .catch((reason) => {
                      if (alive.current) {
                        setError(String(reason));
                        setStarting({ ...turn, failed: true });
                      }
                    })
                    .finally(() => {
                      preparingRef.current = false;
                    });
              },
            },
          }
        : error
          ? {
              text: error,
              action: { label: "Dismiss", run: () => setError("") },
            }
          : catalogProblem
            ? {
                text: `Couldn’t load models from ${machine.name}.`,
                detail: catalogProblem,
                action: {
                  label: "Retry",
                  run: () => setCatalogRefresh((value) => value + 1),
                },
              }
            : undefined;

  const selectWorktree = async (tree: Worktree) => {
    const parsed = parseRemotePath(tree.path);
    if (!parsed || parsed.environmentId !== machine.environmentId)
      throw new Error("Choose a worktree on this machine");
    if (sessionId)
      throw new Error(
        "This session’s worktree is fixed. Start a new session to use another.",
      );
    if (parsed.hostPath === executionCwd) return;
    rememberRemotePendingWorktree(shell.id, parsed.hostPath);
    setSelectedCwd(parsed.hostPath);
  };

  const buildPlan = (
    blockId: string,
    target?: PlanBuildTarget,
  ): boolean | Promise<boolean> => {
    const block = hostSession?.blocks.find(
      (entry) => entry.id === blockId && entry.role === "plan",
    );
    if (!block || !block.text.trim() || block.streaming || busy) return false;
    if (
      target &&
      (target.harness !== configuration.harness ||
        target.model !== configuration.model ||
        !sameModelSettings(target.modelSettings, configuration.settings))
    ) {
      setError(
        "Select that model in the composer before building this remote plan.",
      );
      return false;
    }
    return new Promise((resolve) => {
      if (
        !submit(
          `Build the approved plan:\n\n${block.text}`,
          [],
          { intent: "build" },
          false,
          blockId,
          resolve,
        )
      )
        resolve(false);
    });
  };

  const stopTurn = () => {
    if (hostSession?.busy && snapshot?.runId)
      void run({
        type: "cancel",
        commandId: crypto.randomUUID(),
        sessionId: hostSession.id,
        runId: snapshot.runId,
      });
  };
  const approve = (
    requestId: number,
    decision: Parameters<SessionPaneProps["onApproval"]>[2],
  ) => {
    if (!hostSession || !snapshot?.runId) return;
    void run({
      type: "approve",
      commandId: crypto.randomUUID(),
      sessionId: hostSession.id,
      runId: snapshot.runId,
      requestId,
      decision,
    });
  };
  const answer = (
    requestId: number,
    reply: Parameters<SessionPaneProps["onQuestionReply"]>[2],
  ) => {
    if (!hostSession || !snapshot?.runId) return;
    void run({
      type: "answer",
      commandId: crypto.randomUUID(),
      sessionId: hostSession.id,
      runId: snapshot.runId,
      requestId,
      reply,
    });
  };
  const compact = () => {
    if (!hostSession || busy || pending || changes || !online) return false;
    void run(message(hostSession.id, "/compact"));
    return true;
  };
  const saveDraft = (text: string, attachments: Attachment[]) =>
    submit(text, attachments, undefined, true);
  useEffect(
    () =>
      registerRemoteSessionActions(shell.id, {
        buildPlan,
        submit: (text, attachments, options) =>
          submit(text, attachments, options),
        saveDraft,
        stop: stopTurn,
        compact,
        approve,
        answer,
      }),
    [shell.id, buildPlan, saveDraft, stopTurn, compact, approve, answer],
  );

  const hostFilePath = (path: string) => {
    const existing = parseRemotePath(path);
    if (existing) return path;
    const absolute =
      path.startsWith("/") ||
      path.startsWith("\\\\") ||
      /^[A-Za-z]:[\\/]/.test(path)
        ? path
        : `${executionCwd.replace(/[\\/]+$/, "")}/${path.replace(/^\.\//, "")}`;
    return remotePath(machine.environmentId, absolute);
  };

  const overrides: RemoteSessionOverrides = {
    session,
    remoteSession: true,
    remoteFeatures: {
      attachments: !!descriptor?.capabilities.includes("attachments.upload"),
      plan: !!descriptor?.capabilities.includes("sessions.plan"),
      draft: !!descriptor?.capabilities.includes("sessions.draft"),
    },
    remoteSessionLoading: !!sessionId && !hostSession && !session.blocks.length,
    remoteSessionStarted: !!sessionId,
    allowedModelHarnesses: hostSession
      ? [hostSession.harness]
      : providers.length
        ? providers
        : ["codex", "claude"],
    onSubmit: (_, text, attachments, options) =>
      submit(text, attachments, options),
    onStop: stopTurn,
    onApproval: (_, requestId, decision) => approve(requestId, decision),
    onQuestionReply: (_, requestId, reply) => answer(requestId, reply),
    onCompactContext: compact,
    onModelChange: (_, harness, model) => {
      if (!isRemoteProvider(harness)) return;
      updateConfiguration((current) => ({
        ...current,
        harness,
        model,
        settings: carryModelSettings(
          modelSource.resolve(harness, model).settings ?? [],
          current.settings,
        ),
      }));
    },
    onModelSettingsChange: (_, settings) =>
      updateConfiguration((current) => ({ ...current, settings })),
    onRuntimeModeChange: (_, mode) =>
      updateConfiguration((current) => ({ ...current, mode })),
    onOpenFile: (path) => onOpenFile(hostFilePath(path)),
    onOpenDiff: (path) => onOpenDiff(path ? hostFilePath(path) : undefined),
    // This computer's features do not apply to a host session.
    onCwdChange: noop,
    onBranchChange: () => {
      notifyGitChanged();
    },
    onWorktreeChange: (_, tree) => selectWorktree(tree),
    onWorkspaceModeChange: (_, mode, base) => {
      setDraftWorkspaceMode(mode);
      if (base) setDraftWorktreeBase(base);
    },
    onWorktreeBaseChange: (_, base) => setDraftWorktreeBase(base),
    onManageWorktrees: undefined,
    onSaveDraft: (_, text, attachments) => saveDraft(text, attachments),
    onRemoveDraft: (_, draftBlockId) => {
      if (!hostSession || busy || pending || !online || removingDraft)
        return false;
      setRemovingDraft(draftBlockId);
      if (hostSession.blocks.every((block) => block.id === draftBlockId))
        void discardSession(hostSession.id);
      else
        void run({
          type: "removeDraft",
          commandId: crypto.randomUUID(),
          sessionId: hostSession.id,
          draftBlockId,
        }).then((receipt) => {
          if (!receipt && alive.current) setRemovingDraft(undefined);
        });
      return true;
    },
    onPlaceSessionInFolder: noop,
    onDeleteQueuedMessage: noop,
    onEditQueuedMessage: noop,
    onQueuedMessageEditingChange: noop,
    onSteerQueuedMessage: noop,
    onResumeQueue: noop,
    onUsageLimitResume: noop,
    onUsageLimitResumeAtReset: noop,
    onUsageLimitDismiss: noop,
    onOpenPlan: (_, blockId) => onOpenPlan(shell.id, blockId),
    onBuildPlan: (_, blockId, target) => buildPlan(blockId, target),
    onSecondOpinion: undefined,
    onHandoff: undefined,
    onBtwSubmit: undefined,
    onBtwRetry: undefined,
    onBtwDelete: undefined,
    onBtwModelChange: undefined,
    onNewTerminal: noop,
    onArchiveSession: undefined,
    onDeleteSession: undefined,
    reviewUndoLocked: true,
  };

  return (
    <ModelSourceContext.Provider value={modelSource}>
      <div className="relative flex h-full min-h-0 flex-col">
        {notice ? (
          <div
            role={error ? "alert" : "status"}
            className="flex shrink-0 items-center gap-3 border-b border-stroke px-4 py-2 text-[12px] text-content/65"
          >
            <span className="min-w-0 flex-1 truncate" title={notice.detail}>
              {notice.text}
              {notice.detail ? (
                <span className="text-content/40"> {notice.detail}</span>
              ) : null}
            </span>
            {notice.action ? (
              <button
                type="button"
                disabled={!online && notice.action.label !== "Dismiss"}
                className="shrink-0 rounded-md px-2 py-1 text-content/70 hover:bg-content/8 hover:text-content disabled:opacity-40"
                onClick={notice.action.run}
              >
                {notice.action.label}
              </button>
            ) : null}
          </div>
        ) : null}
        <div className="min-h-0 flex-1">{render(overrides)}</div>
      </div>
    </ModelSourceContext.Provider>
  );
}
