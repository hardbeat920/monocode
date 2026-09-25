import { useEffect, useRef, useState, type ReactNode } from "react";
import { AgentTranscript } from "../../sessions/ui/AgentTranscript";
import { QuestionForm } from "../../sessions/ui/QuestionForm";
import { MODELS } from "../../sessions/model/models";
import {
  RUNTIME_MODES,
  RUNTIME_MODE_LABEL,
  type RuntimeMode,
} from "../../sessions/model/session";
import {
  clearPendingRemoteCommand,
  loadRemoteSession,
  pendingRemoteCommand,
  rememberSession,
  rememberWorkspace,
  rememberedSession,
  remoteDraft,
  remoteRequest,
  savePendingRemoteCommand,
  saveRemoteDraft,
  workspaceFor,
} from "../model/connections";
import {
  requireHostDescriptor,
  type CommandReceipt,
  type HostCommand,
  type HostDescriptor,
  type HostProject,
  type HostSession,
  type HostSessionSummary,
  type RemoteMachine,
  type RemoteProvider,
} from "../model/protocol";

const field =
  "rounded-md border border-content/15 bg-background-base p-2 text-[12px] text-content";

/** Remote views deliberately don't mount local workspace hooks. All execution,
 * approval and file actions below explicitly target their owning machine. */
export function RemoteSessionPane({
  machine,
  project,
  machinePicker,
}: {
  machine: RemoteMachine;
  project: string;
  machinePicker: ReactNode;
}) {
  const [descriptor, setDescriptor] = useState<HostDescriptor>();
  const [workspace, setWorkspace] = useState(() =>
    workspaceFor(project, machine.environmentId),
  );
  const [path, setPath] = useState("");
  const [sessionId, setSessionId] = useState(() =>
    rememberedSession(project, machine.environmentId),
  );
  const [sessions, setSessions] = useState<HostSessionSummary[]>([]);
  const [snapshot, setSnapshot] = useState<HostSession>();
  const [online, setOnline] = useState(false);
  const [error, setError] = useState("");
  const [connectionError, setConnectionError] = useState("");
  const [pending, setPending] = useState(() =>
    pendingRemoteCommand(project, machine.environmentId),
  );
  const [sending, setSending] = useState(false);
  const sendingRef = useRef(false);
  const [provider, setProvider] = useState<RemoteProvider>("codex");
  const [model, setModel] = useState(
    MODELS.find((model) => model.harness === "codex")?.id ?? "codex:gpt-5.4",
  );
  const [mode, setMode] = useState<RuntimeMode>("supervised");
  const draftKey = `${machine.environmentId}:${sessionId ?? "new"}`;
  const [draft, setDraft] = useState(() => remoteDraft(project, draftKey));
  const [preview, setPreview] = useState<{ title: string; text: string }>();
  const snapshotRef = useRef(snapshot);
  snapshotRef.current = snapshot;
  const [refresh, setRefresh] = useState(0);
  const alive = useRef(true);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);
  useEffect(() => {
    setDraft(remoteDraft(project, draftKey));
  }, [project, draftKey]);

  useEffect(() => {
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    let failed = 0;
    // Every request already carries the expected host identity; describe
    // again only after a failure, when the host may have been replaced.
    let described = false;
    const poll = async () => {
      let active = false;
      try {
        if (!described) {
          const host = requireHostDescriptor(
            await remoteRequest<HostDescriptor>(
              machine.id,
              "environment.describe",
            ),
          );
          if (host.environmentId !== machine.environmentId)
            throw new Error(
              "Host identity changed. Reconnect this machine before continuing.",
            );
          if (disposed) return;
          setDescriptor(host);
          described = true;
        }
        const list = workspace
          ? await remoteRequest<HostSessionSummary[]>(
              machine.id,
              "sessions.list",
              { projectId: workspace.id },
            )
          : [];
        const known =
          snapshotRef.current?.session.id === sessionId
            ? snapshotRef.current
            : undefined;
        const next = sessionId
          ? await loadRemoteSession(machine.id, sessionId, known)
          : undefined;
        if (disposed) return;
        if (next && workspace && next.projectId !== workspace.id) {
          throw new Error("This session belongs to a different host workspace");
        }
        setSessions(list);
        setOnline(true);
        setConnectionError("");
        failed = 0;
        setSnapshot(next);
        active =
          !!next?.session.busy ||
          list.some((session) => session.status === "running");
      } catch (reason) {
        if (disposed) return;
        setOnline(false);
        setConnectionError(String(reason));
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
              : 3_000,
        );
    };
    void poll();
    return () => {
      disposed = true;
      clearTimeout(timer);
    };
  }, [machine.id, machine.environmentId, workspace?.id, sessionId, refresh]);

  useEffect(() => {
    if (
      descriptor?.providers.length &&
      !descriptor.providers.includes(provider)
    ) {
      const next = descriptor.providers[0];
      setProvider(next);
      setModel(MODELS.find((model) => model.harness === next)?.id ?? next);
    }
  }, [descriptor, provider]);

  const selectSession = (id: string) => {
    setSnapshot(undefined);
    setSessionId(id || undefined);
    setPreview(undefined);
    rememberSession(project, machine.environmentId, id);
  };

  const run = async (command: HostCommand) => {
    if (sendingRef.current) return;
    sendingRef.current = true;
    setSending(true);
    setError("");
    // Keep the original ID across disconnects and app restarts. An ambiguous
    // response is retried explicitly instead of silently sending a new prompt.
    try {
      savePendingRemoteCommand(project, machine.environmentId, command);
      setPending(command);
      const receipt = await remoteRequest<CommandReceipt>(
        machine.id,
        "commands.dispatch",
        command,
      );
      clearPendingRemoteCommand(
        project,
        machine.environmentId,
        command.commandId,
      );
      if (
        command.type === "send" &&
        remoteDraft(
          project,
          `${machine.environmentId}:${command.sessionId}`,
        ) === command.text
      )
        saveRemoteDraft(
          project,
          `${machine.environmentId}:${command.sessionId}`,
          "",
        );
      if (!alive.current) return;
      setPending(pendingRemoteCommand(project, machine.environmentId));
      if (command.type === "create") selectSession(receipt.sessionId);
      if (command.type === "send")
        setDraft((current) => (current === command.text ? "" : current));
      setRefresh((value) => value + 1);
    } catch (reason) {
      if (!alive.current) return;
      const message = String(reason);
      if (message.includes("Host rejected request:")) {
        clearPendingRemoteCommand(
          project,
          machine.environmentId,
          command.commandId,
        );
        setPending(pendingRemoteCommand(project, machine.environmentId));
      }
      setError(message);
    } finally {
      sendingRef.current = false;
      if (alive.current) setSending(false);
    }
  };

  const attachWorkspace = async () => {
    setSending(true);
    setError("");
    try {
      const workspace = await remoteRequest<HostProject>(
        machine.id,
        "projects.open",
        { cwd: path },
      );
      rememberWorkspace(project, machine.environmentId, workspace);
      if (alive.current) setWorkspace(workspace);
    } catch (reason) {
      if (alive.current) setError(String(reason));
    } finally {
      if (alive.current) setSending(false);
    }
  };
  const inspect = async (method: "git.diff" | "files.read", path?: string) => {
    if (!workspace) return;
    try {
      const text = await remoteRequest<string>(machine.id, method, {
        projectId: workspace.id,
        path,
      });
      if (alive.current)
        setPreview({
          title: path ?? "Changes on host",
          text: text || "No tracked changes against HEAD.",
        });
    } catch (reason) {
      if (alive.current) setError(String(reason));
    }
  };

  const session =
    snapshot && snapshot.session.id === sessionId
      ? snapshot.session
      : undefined;
  const canAct = online && !sending && !pending;
  return (
    <div className="flex h-full min-h-0 flex-col text-content">
      <div className="flex flex-wrap items-center gap-3 border-b border-content/10 px-5 py-3 text-[12px]">
        {machinePicker}
        <span className={online ? "text-emerald-400" : "text-content/40"}>
          {online ? "Connected" : "Reconnecting…"}
        </span>
        {workspace && (
          <span
            className="min-w-0 flex-1 truncate text-content/45"
            title={workspace.cwd}
          >
            {workspace.cwd}
          </span>
        )}
        {workspace && (
          <button
            disabled={!online}
            onClick={() => void inspect("git.diff")}
            className="text-content/60 disabled:opacity-40"
          >
            View changes
          </button>
        )}
      </div>
      {(error || connectionError) && (
        <div
          role="alert"
          className="flex items-center gap-3 px-5 py-2 text-[12px] text-red-400"
        >
          <span className="flex-1">{error || connectionError}</span>
          <button
            onClick={() => {
              setError("");
              setConnectionError("");
              setRefresh((value) => value + 1);
            }}
          >
            Retry connection
          </button>
        </div>
      )}
      {pending && (
        <div className="flex items-center gap-3 bg-content/5 px-5 py-3 text-[12px]">
          <span className="flex-1">
            Waiting to confirm the host accepted your request. Retrying will use
            the same request ID.
          </span>
          <button
            disabled={!online || sending}
            onClick={() => void run(pending)}
          >
            {sending ? "Sending…" : "Retry request"}
          </button>
        </div>
      )}
      {!workspace ? (
        <form
          className="m-auto flex w-full max-w-lg flex-col gap-3 p-6"
          onSubmit={(event) => {
            event.preventDefault();
            void attachWorkspace();
          }}
        >
          <p className="font-medium">Connect this project to {machine.name}</p>
          <p className="text-[13px] text-content/50">
            Enter the existing checkout’s absolute path on that machine. This
            links the project selected in your rail.
          </p>
          <input
            required
            className={field}
            aria-label="Project path on host"
            placeholder={
              descriptor?.platform === "win32"
                ? "C:\\Users\\me\\code\\my-app"
                : "/home/me/projects/my-app"
            }
            value={path}
            onChange={(event) => setPath(event.target.value)}
          />
          <button
            disabled={!online || sending}
            className="rounded-md bg-content/10 px-4 py-2 text-[13px] disabled:opacity-40"
          >
            Link workspace
          </button>
        </form>
      ) : (
        <>
          <div className="flex items-center gap-2 px-5 py-3">
            <select
              aria-label="Remote session"
              className={`${field} min-w-0 flex-1`}
              value={sessionId ?? ""}
              disabled={sending || !!pending}
              onChange={(event) => selectSession(event.target.value)}
            >
              <option value="">New session on {machine.name}</option>
              {sessionId &&
                !sessions.some((session) => session.id === sessionId) && (
                  <option value={sessionId}>Loading session…</option>
                )}
              {sessions.map((session) => (
                <option key={session.id} value={session.id}>
                  {session.title}
                  {session.status === "running"
                    ? " · Running"
                    : session.status === "interrupted"
                      ? " · Interrupted"
                      : ""}
                </option>
              ))}
            </select>
            <button
              className="px-2 text-[12px] text-content/60 disabled:opacity-40"
              disabled={sending || !!pending}
              onClick={() => selectSession("")}
            >
              New session
            </button>
          </div>
          {preview ? (
            <div className="flex min-h-0 flex-1 flex-col px-5 pb-4">
              <div className="flex justify-between py-2 text-[12px]">
                <span>{preview.title}</span>
                <button onClick={() => setPreview(undefined)}>
                  Back to session
                </button>
              </div>
              <pre className="min-h-0 flex-1 overflow-auto rounded-lg bg-content/5 p-4 text-[12px] whitespace-pre-wrap">
                {preview.text}
              </pre>
            </div>
          ) : session ? (
            <div className="min-h-0 flex-1 overflow-auto px-5">
              <AgentTranscript
                blocks={session.blocks}
                busy={online && session.busy}
                harness={session.harness}
                model={session.model}
                cwd={session.cwd}
                pendingQuestion={!!session.pendingQuestion}
                backgroundTasks={session.backgroundTasks}
                onOpenFile={
                  online
                    ? (path) => void inspect("files.read", path)
                    : undefined
                }
                onApproval={
                  canAct
                    ? (requestId, decision) =>
                        void run({
                          type: "approve",
                          commandId: crypto.randomUUID(),
                          sessionId: session.id,
                          runId: snapshot!.runId!,
                          requestId,
                          decision,
                        })
                    : undefined
                }
              />
            </div>
          ) : !sessionId ? (
            <div className="m-auto flex w-full max-w-lg flex-col gap-4 p-6">
              <p className="text-[15px]">Start a session on {machine.name}</p>
              <div className="flex flex-wrap gap-2">
                <select
                  aria-label="Remote provider"
                  className={field}
                  value={provider}
                  onChange={(event) => {
                    const next = event.target.value as RemoteProvider;
                    setProvider(next);
                    setModel(
                      MODELS.find((model) => model.harness === next)?.id ??
                        next,
                    );
                  }}
                >
                  {(descriptor?.providers ?? []).map((provider) => (
                    <option key={provider} value={provider}>
                      {provider === "codex" ? "Codex" : "Claude Code"}
                    </option>
                  ))}
                </select>
                <input
                  className={`${field} min-w-0 flex-1`}
                  aria-label="Remote model"
                  list="remote-models"
                  value={model}
                  onChange={(event) => setModel(event.target.value)}
                />
                <datalist id="remote-models">
                  {MODELS.filter((model) => model.harness === provider).map(
                    (model) => (
                      <option key={model.id} value={model.id}>
                        {model.name}
                      </option>
                    ),
                  )}
                </datalist>
              </div>
              <select
                aria-label="Remote permission mode"
                className={field}
                value={mode}
                onChange={(event) => setMode(event.target.value as RuntimeMode)}
              >
                {RUNTIME_MODES.map((mode) => (
                  <option key={mode} value={mode}>
                    {RUNTIME_MODE_LABEL[mode]}
                  </option>
                ))}
              </select>
              <p className="text-[12px] text-content/45">
                Uses the current checkout and provider account on the host.
                Sessions keep running when you close this app.
              </p>
              {!descriptor?.providers.length && (
                <p className="text-[12px] text-content/50">
                  Install and authenticate Codex or Claude Code on the host,
                  then restart the host.
                </p>
              )}
              <button
                disabled={
                  !canAct ||
                  !descriptor?.providers.includes(provider) ||
                  !model.trim()
                }
                className="rounded-md bg-content/10 p-2 text-[13px] disabled:opacity-40"
                onClick={() =>
                  void run({
                    type: "create",
                    commandId: crypto.randomUUID(),
                    projectId: workspace.id,
                    harness: provider,
                    model,
                    runtimeMode: mode,
                  })
                }
              >
                Create session
              </button>
            </div>
          ) : (
            <div className="m-auto text-[13px] text-content/40">
              Loading session from host…
            </div>
          )}
          {session && (
            <div className="mx-auto w-full max-w-4xl p-4">
              {session.pendingQuestion && canAct && (
                <QuestionForm
                  prompt={session.pendingQuestion}
                  onReply={(requestId, reply) =>
                    void run({
                      type: "answer",
                      commandId: crypto.randomUUID(),
                      sessionId: session.id,
                      runId: snapshot!.runId!,
                      requestId,
                      reply,
                    })
                  }
                />
              )}
              <form
                className="rounded-lg border border-content/15 bg-content/3 p-3"
                onSubmit={(event) => {
                  event.preventDefault();
                  if (canAct && !session.busy && draft.trim())
                    void run({
                      type: "send",
                      commandId: crypto.randomUUID(),
                      sessionId: session.id,
                      text: draft,
                    });
                }}
              >
                <textarea
                  aria-label="Message remote agent"
                  placeholder={
                    session.busy
                      ? "Draft a follow-up…"
                      : "What should the agent do?"
                  }
                  rows={3}
                  className="w-full resize-none bg-transparent text-[13px] outline-none"
                  value={draft}
                  onChange={(event) => {
                    setDraft(event.target.value);
                    saveRemoteDraft(project, draftKey, event.target.value);
                  }}
                  onKeyDown={(event) => {
                    if (
                      event.key === "Enter" &&
                      !event.shiftKey &&
                      !event.nativeEvent.isComposing
                    ) {
                      event.preventDefault();
                      event.currentTarget.form?.requestSubmit();
                    }
                  }}
                />
                <div className="flex items-center justify-between text-[12px]">
                  <span className="text-content/40">
                    {machine.name} · {session.harness} ·{" "}
                    {session.busy
                      ? online
                        ? "Running on host"
                        : "Last seen running"
                      : snapshot?.status === "interrupted"
                        ? "Interrupted"
                        : "Ready"}
                  </span>
                  {session.busy ? (
                    <button
                      type="button"
                      disabled={!canAct}
                      className="rounded bg-content/10 px-3 py-1.5 disabled:opacity-40"
                      onClick={() =>
                        void run({
                          type: "cancel",
                          commandId: crypto.randomUUID(),
                          sessionId: session.id,
                          runId: snapshot!.runId!,
                        })
                      }
                    >
                      Stop
                    </button>
                  ) : (
                    <button
                      disabled={!canAct || !draft.trim()}
                      className="rounded bg-content/10 px-3 py-1.5 disabled:opacity-40"
                    >
                      Send
                    </button>
                  )}
                </div>
              </form>
            </div>
          )}
        </>
      )}
    </div>
  );
}
