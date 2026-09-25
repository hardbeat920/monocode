import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import { Globe, Loader, Plus, Trash2 } from "../../../shared/ui/icons";
import {
  connectMachine,
  disconnectMachine,
  refreshRemoteMachines,
  remoteRequest,
  useRemoteMachines,
} from "../model/connections";
import type {
  HostDescriptor,
  RemoteMachine,
  SshSetup,
} from "../model/protocol";

const input =
  "w-full rounded-lg border border-content/15 bg-content/3 px-3 py-2 text-[13px] outline-none focus:border-content/35";
const button =
  "rounded-lg bg-selection px-3 py-2 text-[13px] font-medium hover:bg-selection-hover disabled:opacity-40";

export function ConnectionsSettings() {
  const { machines, loaded } = useRemoteMachines();
  const [adding, setAdding] = useState(false);
  const [name, setName] = useState("");
  const [target, setTarget] = useState("");
  const [port, setPort] = useState("");
  const [jobId, setJobId] = useState<string>();
  const [job, setJob] = useState<SshSetup>();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [answer, setAnswer] = useState("");
  const [answering, setAnswering] = useState(false);
  const [status, setStatus] = useState<Record<string, string>>({});
  const [url, setUrl] = useState("http://127.0.0.1:3774");
  const [token, setToken] = useState("");
  const alive = useRef(true);
  const currentJob = useRef<string | undefined>(undefined);
  const submitting = useRef(false);
  const progress = useRef<HTMLDivElement>(null);
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
              `${next.machine.name} is connected. Choose it from the machine selector in a new session.`,
            );
            setStatus((current) => ({
              ...current,
              [next.machine!.id]: "Connected",
            }));
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
  }, [jobId]);
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
            let label = "Connected";
            try {
              const host = await remoteRequest<HostDescriptor>(
                machine.id,
                "environment.describe",
              );
              if (host.environmentId !== machine.environmentId)
                throw new Error("Host identity changed");
              if (!host.providers.length)
                label = "Connected · install Codex or Claude on the host";
            } catch {
              label = "Offline · reconnect to check access";
            }
            if (!disposed)
              setStatus((current) => ({ ...current, [machine.id]: label }));
          }),
        );
      if (!disposed) timer = setTimeout(() => void check(), 10_000);
    };
    void check();
    return () => {
      disposed = true;
      clearTimeout(timer);
    };
  }, [machines, busy]);
  const begin = async (machine?: RemoteMachine) => {
    if (submitting.current) return;
    submitting.current = true;
    setBusy(true);
    setError("");
    setNotice("");
    setJob(undefined);
    try {
      const id = machine
        ? await invoke<string>("remote_ssh_reconnect", {
            machineId: machine.id,
          })
        : await invoke<string>("remote_ssh_begin", {
            target: target.trim(),
            name: name.trim(),
            port: port ? Number(port) : null,
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
  const remove = async (machine: RemoteMachine) => {
    setError("");
    try {
      await disconnectMachine(machine.id);
    } catch (reason) {
      setError(String(reason));
    }
  };
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
          {machines.map((machine) => (
            <div key={machine.id} className="flex items-center gap-3 px-4 py-4">
              <Globe className="size-5 shrink-0 text-content/45" />
              <div className="min-w-0 flex-1">
                <div className="truncate text-[13px] font-medium">
                  {machine.name}
                </div>
                <div className="mt-1 truncate text-[12px] text-content/45">
                  {machine.ssh
                    ? `SSH · ${machine.ssh.target}${machine.ssh.port ? ` · port ${machine.ssh.port}` : ""}`
                    : machine.endpoint}
                </div>
                <div className="mt-1 text-[12px] text-content/50">
                  {status[machine.id] ?? "Checking connection…"}
                </div>
              </div>
              {machine.ssh && (
                <button
                  className={button}
                  disabled={busy}
                  onClick={() => void begin(machine)}
                >
                  Reconnect
                </button>
              )}
              <button
                disabled={busy}
                className="rounded p-2 text-content/40 hover:bg-selection hover:text-content disabled:opacity-40"
                aria-label={`Remove ${machine.name}`}
                title="Remove connection; remote sessions keep running"
                onClick={() => void remove(machine)}
              >
                <Trash2 className="size-4" />
              </button>
            </div>
          ))}
        </div>
      ) : loaded && !adding ? (
        <div className="rounded-xl border border-dashed border-content/15 px-5 py-8 text-center text-[13px] text-content/45">
          Add your always-on Windows, Mac, or Linux machine to get started.
        </div>
      ) : null}
      {adding && (
        <form
          className="flex flex-col gap-4 rounded-xl border border-stroke p-5"
          onSubmit={(event) => {
            event.preventDefault();
            void begin();
          }}
        >
          <div className="flex items-center justify-between">
            <h3 className="text-[14px] font-medium">Connect through SSH</h3>
            <span className="rounded bg-selection px-2 py-1 text-[11px] text-content/60">
              SSH
            </span>
          </div>
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
          <label className="flex flex-col gap-1.5 text-[12px] text-content/65">
            Name <span className="sr-only">(optional)</span>
            <input
              disabled={busy}
              className={input}
              value={name}
              onChange={(event) => setName(event.target.value)}
              placeholder="Optional, e.g. Home Mac mini"
            />
          </label>
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
            MonoCode installs and starts its background host, then connects
            securely. Your SSH keys and config are used automatically. Enable
            SSH on the host and sign in to Codex or Claude Code there. On
            Windows and Mac, keep the host’s desktop account signed in and the
            machine awake. Locking the desktop is fine.
          </p>
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
              {busy ? "Connecting…" : "Connect"}
            </button>
          </div>
        </form>
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
        <p
          role="alert"
          className="whitespace-pre-wrap break-words rounded-lg bg-red-500/5 p-3 text-[12px] leading-relaxed text-red-400"
        >
          {error}
        </p>
      )}
      {notice && (
        <p role="status" className="text-[13px] text-emerald-500">
          {notice}
        </p>
      )}
      <details className="text-[12px] text-content/45">
        <summary className="cursor-pointer">
          Connect to an existing host by URL
        </summary>
        <form
          className="mt-4 flex flex-col gap-3"
          onSubmit={(event) => {
            event.preventDefault();
            if (busy) return;
            setBusy(true);
            setError("");
            void connectMachine("", url, token)
              .then((machine) => {
                setToken("");
                setNotice(`${machine.name} is connected.`);
              })
              .catch((reason) => setError(String(reason)))
              .finally(() => setBusy(false));
          }}
        >
          <label>
            Host URL
            <input
              required
              disabled={busy}
              className={`${input} mt-1`}
              value={url}
              onChange={(event) => setUrl(event.target.value)}
            />
          </label>
          <label>
            Device token
            <input
              required
              disabled={busy}
              type="password"
              autoComplete="off"
              className={`${input} mt-1`}
              value={token}
              onChange={(event) => setToken(event.target.value)}
            />
          </label>
          <button className={`${button} self-start`} disabled={busy}>
            Connect by URL
          </button>
        </form>
      </details>
    </div>
  );
}
