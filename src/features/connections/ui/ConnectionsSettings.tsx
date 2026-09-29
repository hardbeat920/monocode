import { invoke } from "@tauri-apps/api/core";
import { getVersion } from "@tauri-apps/api/app";
import { useEffect, useRef, useState } from "react";
import { Internet, Loader, Plus, Trash2 } from "../../../shared/ui/icons";
import {
  disconnectMachine,
  pairMachine,
  refreshRemoteMachines,
  remoteRequest,
  retryMachine,
  useRemoteMachines,
} from "../model/connections";
import {
  hostConnectCommand,
  hostNeedsUpdate,
  REMOTE_PROVIDERS,
  type HostDescriptor,
  type RemoteMachine,
  type SshSetup,
} from "../model/protocol";

const input =
  "w-full rounded-lg border border-content/15 bg-content/3 px-3 py-2 text-[13px] outline-none focus:border-content/35";
const button =
  "rounded-lg bg-selection px-3 py-2 text-[13px] font-medium hover:bg-selection-hover disabled:opacity-40";
const code = "rounded bg-content/10 px-1";

/** How the desktop reaches a machine: its first direct address, the SSH
 * forward, or both. */
function routeLabel(machine: RemoteMachine): string {
  const ssh = machine.ssh
    ? `${machine.ssh.target}${machine.ssh.port ? ` · port ${machine.ssh.port}` : ""}`
    : "";
  const direct = machine.endpoints?.[0]?.replace(/^https:\/\//, "");
  if (direct) {
    const more =
      machine.endpoints!.length > 1 ? ` (+${machine.endpoints!.length - 1})` : "";
    return ssh ? `${direct}${more} · SSH fallback ${ssh}` : `${direct}${more}`;
  }
  return ssh ? `SSH · ${ssh}` : machine.endpoint;
}

type MachineStatus = { label: string; host?: HostDescriptor; offline?: boolean };

const connectedNotice = (machine: RemoteMachine) =>
  `${machine.name} is connected. To work on it, click + next to Projects in the project rail and choose Open folder on a machine.`;

export function ConnectionsSettings() {
  const { machines, loaded } = useRemoteMachines();
  const [adding, setAdding] = useState(false);
  const [mode, setMode] = useState<"link" | "ssh">("link");
  const [link, setLink] = useState("");
  const [name, setName] = useState("");
  const [target, setTarget] = useState("");
  const [port, setPort] = useState("");
  const [version, setVersion] = useState<string>();
  const [copied, setCopied] = useState(false);
  const [jobId, setJobId] = useState<string>();
  const [job, setJob] = useState<SshSetup>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [answer, setAnswer] = useState("");
  const [answering, setAnswering] = useState(false);
  const [status, setStatus] = useState<Record<string, MachineStatus>>({});
  const [updatingMachine, setUpdatingMachine] = useState<string>();
  const [removing, setRemoving] = useState<string>();
  const [revoking, setRevoking] = useState(false);
  const [checks, setChecks] = useState(0);
  const alive = useRef(true);
  const currentJob = useRef<string | undefined>(undefined);
  const submitting = useRef(false);
  const progress = useRef<HTMLDivElement>(null);
  const command = hostConnectCommand(version);
  useEffect(() => {
    void getVersion()
      .then((value) => alive.current && setVersion(value))
      .catch(() => {});
  }, []);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
      if (currentJob.current)
        void invoke("remote_ssh_cancel", { jobId: currentJob.current }).catch(
          () => {},
        );
    };
  }, []);
  useEffect(() => {
    if (!jobId) return;
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    const poll = async () => {
      try {
        const next = await invoke<SshSetup>("remote_ssh_poll", { jobId });
        if (disposed) return;
        setJob(next);
        if (next.done) {
          currentJob.current = undefined;
          submitting.current = false;
          setBusy(false);
          setJobId(undefined);
          setAnswer("");
          if (next.error) setError(next.error);
          else if (next.machine) {
            setAdding(false);
            setTarget("");
            setName("");
            setPort("");
            setNotice(
              updatingMachine
                ? `${next.machine.name} was updated and reconnected.`
                : connectedNotice(next.machine),
            );
            setUpdatingMachine(undefined);
            setChecks((value) => value + 1);
            refreshRemoteMachines();
          }
          return;
        }
      } catch (reason) {
        if (disposed) return;
        setError(String(reason));
        void invoke("remote_ssh_cancel", { jobId }).catch(() => {});
        currentJob.current = undefined;
        submitting.current = false;
        setBusy(false);
        setJobId(undefined);
        return;
      }
      timer = setTimeout(() => void poll(), 350);
    };
    void poll();
    return () => {
      disposed = true;
      clearTimeout(timer);
    };
  }, [jobId, updatingMachine]);
  useEffect(() => {
    setAnswer("");
    setAnswering(false);
    if (job?.prompt)
      progress.current?.scrollIntoView?.({
        block: "nearest",
        behavior: "smooth",
      });
  }, [job?.prompt?.id]);
  useEffect(() => {
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    const check = async () => {
      if (!busy)
        await Promise.all(
          machines.map(async (machine) => {
            let next: MachineStatus;
            try {
              const host = await remoteRequest<HostDescriptor>(
                machine.id,
                "environment.describe",
                { supportedProviders: REMOTE_PROVIDERS },
              );
              if (host.environmentId !== machine.environmentId)
                throw new Error("Host identity changed");
              const parts = [
                `Connected · host ${host.hostVersion ?? "older version"}`,
              ];
              if (!host.providers.length)
                parts.push("install a supported provider on the host");
              if (hostNeedsUpdate(host, version)) parts.push("update available");
              next = { label: parts.join(" · "), host };
            } catch (reason) {
              next = {
                label: `Offline · ${String(reason).replace(/^Error: /, "")}`,
                offline: true,
              };
            }
            if (!disposed)
              setStatus((current) => ({ ...current, [machine.id]: next }));
          }),
        );
      if (!disposed) timer = setTimeout(() => void check(), 10_000);
    };
    void check();
    return () => {
      disposed = true;
      clearTimeout(timer);
    };
  }, [machines, busy, version, checks]);

  const begin = async (machine?: RemoteMachine, upgrade = false) => {
    if (submitting.current) return;
    submitting.current = true;
    setBusy(true);
    setError("");
    setNotice("");
    setJob(undefined);
    setUpdatingMachine(upgrade ? machine?.id : undefined);
    try {
      const id = machine
        ? await invoke<string>("remote_ssh_reconnect", {
            machineId: machine.id,
            ...(upgrade ? { upgrade: true } : {}),
          })
        : await invoke<string>("remote_ssh_begin", {
            target: target.trim(),
            name: name.trim(),
            port: port ? Number(port) : null,
            ...(upgrade ? { upgrade: true } : {}),
          });
      if (!alive.current) {
        await invoke("remote_ssh_cancel", { jobId: id });
        return;
      }
      currentJob.current = id;
      setJobId(id);
    } catch (reason) {
      submitting.current = false;
      if (alive.current) {
        setError(String(reason));
        setBusy(false);
      }
    }
  };
  const pair = async () => {
    if (busy || !link.trim()) return;
    setBusy(true);
    setError("");
    setNotice("");
    try {
      const machine = await pairMachine(link.trim(), name.trim());
      if (!alive.current) return;
      setLink("");
      setName("");
      setAdding(false);
      setNotice(connectedNotice(machine));
      setChecks((value) => value + 1);
    } catch (reason) {
      if (alive.current) setError(String(reason));
    } finally {
      if (alive.current) setBusy(false);
    }
  };
  const retry = async (machine: RemoteMachine) => {
    setError("");
    setStatus((current) => ({
      ...current,
      [machine.id]: { label: "Checking connection…" },
    }));
    await retryMachine(machine.id).catch(() => {});
    setChecks((value) => value + 1);
  };
  const respond = async (value: string) => {
    if (!jobId || !job?.prompt || answering) return;
    setAnswering(true);
    setError("");
    try {
      await invoke("remote_ssh_answer", {
        jobId,
        promptId: job.prompt.id,
        answer: value,
      });
      setAnswer("");
    } catch (reason) {
      setError(String(reason));
      setAnswering(false);
    }
  };
  const remove = async (machine: RemoteMachine, revoke: boolean) => {
    setError("");
    setNotice("");
    setRevoking(true);
    try {
      if (revoke) {
        try {
          await remoteRequest(machine.id, "devices.revokeSelf");
        } catch (reason) {
          throw new Error(
            `Could not revoke access, so ${machine.name} was not removed: ${String(reason)}. Reconnect and try again, or remove it from this desktop only and revoke it on the host with monocode-host devices and monocode-host revoke <device-id>.`,
          );
        }
      }
      await disconnectMachine(machine.id);
      setRemoving(undefined);
      setNotice(
        revoke
          ? `${machine.name} was removed and this desktop's access was revoked. The host and its sessions keep running.`
          : `${machine.name} was removed from this desktop. The host and its sessions keep running, and it still accepts this desktop's credential.`,
      );
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      if (alive.current) setRevoking(false);
    }
  };
  const copyCommand = async () => {
    try {
      await navigator.clipboard.writeText(command);
      setCopied(true);
      setTimeout(() => alive.current && setCopied(false), 1500);
    } catch {
      /* the command stays selectable */
    }
  };
  // Setup stops at an older host with running turns until the user agrees
  // to restart it.
  const upgradeBlocked = error.includes("--yes");
  return (
    <div data-setting-id="remote-machines" className="flex flex-col gap-5">
      <div className="flex items-end justify-between gap-4">
        <div className="min-w-0">
          <h2 className="text-[13px] font-semibold text-content">
            Your machines
          </h2>
          <p className="mt-1 text-[12px] leading-relaxed text-content/45">
            Run agents on another computer and return to them from your laptop.
            The host keeps working when you close MonoCode here.
          </p>
        </div>
        {!adding && (
          <button
            className={`${button} flex shrink-0 items-center gap-2`}
            disabled={busy}
            onClick={() => {
              setAdding(true);
              setError("");
              setNotice("");
            }}
          >
            <Plus className="size-4" /> Add machine
          </button>
        )}
      </div>
      {machines.length > 0 ? (
        <div className="divide-y divide-stroke overflow-hidden rounded-xl border border-stroke">
          {machines.map((machine) => {
            const state = status[machine.id];
            const update = !!state?.host && hostNeedsUpdate(state.host, version);
            return (
              <div key={machine.id}>
                <div className="flex items-center gap-3 px-4 py-4">
                  <Internet className="size-5 shrink-0 text-content/45" />
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-[13px] font-medium">
                      {machine.name}
                    </div>
                    <div className="mt-1 truncate text-[12px] text-content/45">
                      {routeLabel(machine)}
                    </div>
                    <div
                      className="mt-1 line-clamp-3 break-words text-[12px] text-content/50"
                      title={state?.label}
                    >
                      {state?.label ?? "Checking connection…"}
                    </div>
                    {update ? (
                      <div className="mt-1 text-[11px] text-content/45">
                        {machine.ssh ? (
                          "Updating restarts the host and interrupts active agent turns."
                        ) : (
                          <>
                            To update, run{" "}
                            <code className={code}>{command}</code> on the
                            machine. It restarts the host and interrupts active
                            agent turns.
                          </>
                        )}
                      </div>
                    ) : null}
                  </div>
                  <div className="flex shrink-0 items-center gap-2">
                    {machine.ssh && update ? (
                      <button
                        className={button}
                        disabled={busy}
                        title="Installs this desktop's host version over SSH and restarts the host; active agent turns will be interrupted"
                        onClick={() => void begin(machine, true)}
                      >
                        Update Host
                      </button>
                    ) : null}
                    {machine.ssh ? (
                      <button
                        className={button}
                        disabled={busy}
                        title="Starts the host over SSH if it stopped, then reconnects"
                        onClick={() => void begin(machine)}
                      >
                        Reconnect
                      </button>
                    ) : state?.offline ? (
                      <button
                        className={button}
                        disabled={busy}
                        onClick={() => void retry(machine)}
                      >
                        Retry
                      </button>
                    ) : null}
                  </div>
                  <button
                    disabled={busy || revoking}
                    className="rounded p-2 text-content/40 hover:bg-selection hover:text-content disabled:opacity-40"
                    aria-label={`Remove ${machine.name}`}
                    title="Remove connection…"
                    onClick={() => {
                      setError("");
                      setRemoving(machine.id);
                    }}
                  >
                    <Trash2 className="size-4" />
                  </button>
                </div>
                {removing === machine.id && (
                  <div
                    role="group"
                    aria-label={`Confirm removing ${machine.name}`}
                    className="flex flex-col gap-3 border-t border-stroke bg-content/3 px-4 py-4 text-[12px] leading-relaxed text-content/60"
                  >
                    <p className="text-[13px] font-medium text-content">
                      Remove {machine.name} from this desktop?
                    </p>
                    <p>
                      This closes this desktop’s connection to the machine. It
                      does not stop the host, and its sessions keep running and
                      stay on that machine. You can pair it again later.
                    </p>
                    <p>
                      Removing alone leaves this desktop’s credential valid on
                      the host. Revoke access to invalidate it first; the
                      machine must be reachable.
                    </p>
                    <p>
                      To stop the host and turn off its background service, run{" "}
                      <code className={code}>
                        ~/.monocode-host/bin/monocode-host service uninstall
                      </code>{" "}
                      on that machine (
                      <code className={code}>
                        %USERPROFILE%\.monocode-host\bin\monocode-host.cmd
                        service uninstall
                      </code>{" "}
                      on Windows). Its sessions and history are kept.
                    </p>
                    <div className="flex flex-wrap gap-2">
                      <button
                        className={button}
                        disabled={revoking}
                        onClick={() => void remove(machine, true)}
                      >
                        Revoke access and remove
                      </button>
                      <button
                        className={button}
                        disabled={revoking}
                        onClick={() => void remove(machine, false)}
                      >
                        Remove from this desktop only
                      </button>
                      <button
                        className="px-3 py-2 text-[13px] text-content/50"
                        disabled={revoking}
                        onClick={() => setRemoving(undefined)}
                      >
                        Cancel
                      </button>
                    </div>
                  </div>
                )}
              </div>
            );
          })}
        </div>
      ) : loaded && !adding ? (
        <div className="rounded-xl border border-dashed border-content/15 px-5 py-8 text-center text-[13px] text-content/45">
          Add your always-on Windows, Mac, or Linux machine to get started.
        </div>
      ) : null}
      {adding && (
        <div className="flex flex-col gap-4 rounded-xl border border-stroke p-5">
          <div className="flex items-center justify-between gap-3">
            <h3 className="text-[14px] font-medium">Add a machine</h3>
            <div
              role="tablist"
              aria-label="How to add the machine"
              className="flex rounded-lg bg-content/5 p-0.5 text-[12px]"
            >
              {(
                [
                  ["link", "Pairing link"],
                  ["ssh", "SSH"],
                ] as const
              ).map(([value, label]) => (
                <button
                  key={value}
                  role="tab"
                  aria-selected={mode === value}
                  disabled={busy}
                  className={`rounded-md px-3 py-1 ${mode === value ? "bg-selection font-medium text-content" : "text-content/55"}`}
                  onClick={() => {
                    setMode(value);
                    setError("");
                  }}
                >
                  {label}
                </button>
              ))}
            </div>
          </div>
          {mode === "link" ? (
            <form
              className="flex flex-col gap-4"
              onSubmit={(event) => {
                event.preventDefault();
                void pair();
              }}
            >
              <div className="flex flex-col gap-1.5 text-[12px] text-content/65">
                1. On the machine, run
                <div className="flex items-center gap-2">
                  <code className="min-w-0 flex-1 select-all truncate rounded-lg bg-content/5 px-3 py-2 font-mono text-[12px] text-content">
                    {command}
                  </code>
                  <button
                    type="button"
                    className={button}
                    onClick={() => void copyCommand()}
                  >
                    {copied ? "Copied" : "Copy"}
                  </button>
                </div>
                <span className="text-content/45">
                  It installs MonoCode Host as a background service, listens on
                  port 3774 over TLS, and prints a one-time pairing link. It
                  needs Node.js 22.13 or newer.
                </span>
              </div>
              <label className="flex flex-col gap-1.5 text-[12px] text-content/65">
                2. Paste the pairing link
                <input
                  autoFocus
                  required
                  disabled={busy}
                  className={`${input} font-mono`}
                  value={link}
                  onChange={(event) => setLink(event.target.value)}
                  placeholder="monocode://pair?..."
                  aria-label="Pairing link"
                  autoComplete="off"
                  spellCheck={false}
                />
              </label>
              <NameField value={name} onChange={setName} disabled={busy} />
              <p className="text-[12px] leading-relaxed text-content/45">
                The link works once, for 15 minutes, and pins the host’s
                certificate. This computer must reach one of the machine’s
                addresses, such as on the same network or tailnet. Otherwise,
                use SSH.
              </p>
              <Requirements />
              <div className="flex justify-end gap-2">
                <button
                  type="button"
                  disabled={busy}
                  className="px-3 py-2 text-[13px] text-content/50"
                  onClick={() => setAdding(false)}
                >
                  Cancel
                </button>
                <button className={button} disabled={busy || !link.trim()}>
                  {busy ? "Pairing…" : "Pair"}
                </button>
              </div>
            </form>
          ) : (
            <form
              className="flex flex-col gap-4"
              onSubmit={(event) => {
                event.preventDefault();
                void begin();
              }}
            >
              <label className="flex flex-col gap-1.5 text-[12px] text-content/65">
                SSH address
                <input
                  autoFocus
                  required
                  disabled={busy}
                  className={input}
                  value={target}
                  onChange={(event) => setTarget(event.target.value)}
                  placeholder="user@my-mac-mini or an SSH alias"
                  autoComplete="off"
                  spellCheck={false}
                />
              </label>
              <NameField value={name} onChange={setName} disabled={busy} />
              <details className="text-[12px] text-content/50">
                <summary className="cursor-pointer">Advanced</summary>
                <label className="mt-3 flex max-w-40 flex-col gap-1.5">
                  SSH port
                  <input
                    disabled={busy}
                    type="number"
                    min={1}
                    max={65535}
                    className={input}
                    value={port}
                    onChange={(event) => setPort(event.target.value)}
                    placeholder="From SSH config"
                  />
                </label>
              </details>
              <p className="text-[12px] leading-relaxed text-content/45">
                MonoCode installs the host package for this desktop’s version
                over SSH, runs its <code className={code}>connect</code>{" "}
                command, and pairs this desktop. The package includes Node, and
                your SSH keys and config are used automatically. When this
                computer can’t reach the machine’s network addresses, MonoCode
                connects through an SSH forward instead.
              </p>
              <Requirements />
              <div className="flex justify-end gap-2">
                <button
                  type="button"
                  disabled={busy}
                  className="px-3 py-2 text-[13px] text-content/50"
                  onClick={() => setAdding(false)}
                >
                  Cancel
                </button>
                <button className={button} disabled={busy || !target.trim()}>
                  {busy ? "Setting up…" : "Set up over SSH"}
                </button>
              </div>
            </form>
          )}
        </div>
      )}
      {busy && jobId && (
        <div
          className="flex flex-col gap-3 rounded-xl border border-stroke p-5"
          role="status"
          ref={progress}
        >
          <div className="flex items-center gap-2 text-[13px]">
            <Loader className="size-4 animate-spin" />
            {job?.message ?? "Starting connection…"}
          </div>
          {job?.prompt && (
            <form
              className="flex flex-col gap-3"
              onSubmit={(event) => {
                event.preventDefault();
                void respond(job.prompt!.confirm ? "yes" : answer);
              }}
            >
              <p className="whitespace-pre-wrap break-words text-[12px] leading-relaxed text-content/70">
                {job.prompt.message}
              </p>
              {!job.prompt.confirm && (
                <input
                  key={job.prompt.id}
                  autoFocus
                  type="password"
                  aria-label="SSH password or passphrase"
                  autoComplete="off"
                  disabled={answering}
                  className={input}
                  value={answer}
                  onChange={(event) => setAnswer(event.target.value)}
                />
              )}
              <div className="flex gap-2">
                <button className={button} disabled={answering}>
                  {job.prompt.confirm ? "Trust host and continue" : "Continue"}
                </button>
                {job.prompt.confirm && (
                  <button
                    type="button"
                    className={button}
                    disabled={answering}
                    onClick={() => void respond("no")}
                  >
                    Reject
                  </button>
                )}
              </div>
            </form>
          )}
          <button
            type="button"
            className="self-start text-[12px] text-content/50 hover:text-content"
            onClick={() => {
              if (jobId)
                void invoke("remote_ssh_cancel", { jobId }).catch((reason) =>
                  setError(String(reason)),
                );
            }}
          >
            Cancel connection
          </button>
        </div>
      )}
      {error && (
        <div
          role="alert"
          className="flex flex-col gap-2 rounded-lg bg-red-500/5 p-3 text-[12px] leading-relaxed text-red-400"
        >
          <p className="whitespace-pre-wrap break-words">{error}</p>
          {upgradeBlocked && adding && mode === "ssh" ? (
            <button
              className={`${button} self-start text-content`}
              disabled={busy}
              onClick={() => void begin(undefined, true)}
            >
              Update and restart the host
            </button>
          ) : null}
        </div>
      )}
      {notice && (
        <p role="status" className="text-[13px] text-emerald-500">
          {notice}
        </p>
      )}
    </div>
  );
}

function NameField({
  value,
  onChange,
  disabled,
}: {
  value: string;
  onChange: (value: string) => void;
  disabled: boolean;
}) {
  return (
    <label className="flex flex-col gap-1.5 text-[12px] text-content/65">
      Name <span className="sr-only">(optional)</span>
      <input
        disabled={disabled}
        className={input}
        value={value}
        onChange={(event) => onChange(event.target.value)}
        placeholder="Optional, e.g. Home Mac mini"
        autoComplete="off"
        autoCorrect="off"
        autoCapitalize="off"
        spellCheck={false}
      />
    </label>
  );
}

function Requirements() {
  return (
    <p className="text-[12px] leading-relaxed text-content/45">
      Sign in to a supported provider, such as Codex or Claude Code, on the machine as the same user. On
      Linux, the host runs as a systemd user service and setup turns on
      lingering for your account (
      <code className={code}>loginctl enable-linger</code>), so agents keep
      running after you log out. On Windows and Mac, keep the machine’s desktop
      account signed in and the machine awake. Locking the desktop is fine.
    </p>
  );
}
