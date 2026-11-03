import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState, type RefObject } from "react";
import { createPortal } from "react-dom";
import {
  Check,
  ChevronDown,
  CircleAlert,
  Loader,
} from "../../../shared/ui/icons";
import { useLockOverscroll } from "../../../shared/hooks/useLockOverscroll";
import { Modal } from "../../../shared/ui/Modal";
import { HarnessIcon } from "../../sessions/ui/HarnessIcon";
import {
  sessionDisplayTitle,
  type Session,
} from "../../sessions/model/session";
import {
  emptyPressure,
  samplePressure,
  type PressureState,
  type ResourceSample,
} from "../model/resourcePressure";

type OwnedProcess = {
  pid: number;
  identity: string;
  sessionId: string;
  cwd: string;
  command: string;
  cpuPercent: number;
  memoryBytes: number;
  agent: boolean;
};
type Props = {
  sessions: RefObject<Session[]>;
  onStopAgent: (id: string) => void | Promise<unknown>;
  onStopSession: (id: string) => void | Promise<unknown>;
  onPressureChange?: (active: boolean) => void;
  /** The footer's slot. Without one, monitoring continues without a notice. */
  chipHost?: HTMLElement | null;
  /** Pill-shaped chip, for the inset layout's footer. */
  insetChip?: boolean;
};

/** Local subscriptions keep resource sampling out of the workspace render loop. */
export function ResourcePressureNotice({
  sessions,
  onStopAgent,
  onStopSession,
  onPressureChange,
  chipHost,
  insetChip = false,
}: Props) {
  const [pressure, setPressure] = useState<PressureState>(emptyPressure);
  const [open, setOpen] = useState(false);
  const [processes, setProcesses] = useState<OwnedProcess[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const [stopError, setStopError] = useState("");
  const [stopping, setStopping] = useState<string>();
  const lockOverscroll = useLockOverscroll<HTMLDivElement>();
  useEffect(() => {
    onPressureChange?.(pressure.active);
  }, [pressure.active, onPressureChange]);

  useEffect(() => {
    if (!chipHost) setOpen(false);
  }, [chipHost]);

  useEffect(() => {
    let alive = true;
    let timer = 0;
    let lastTick = performance.now();
    let stalls = 0;
    let lastPoll = lastTick;
    let state = emptyPressure();
    let polling = false;
    const tick = () => {
      const now = performance.now();
      if (document.hidden) {
        stalls = 0;
        lastPoll = now;
        state = { ...state, high: 0, low: 0 };
      } else {
        const delay = now - lastTick - 500;
        if (delay > 100) stalls += delay;
        if (!polling && now - lastPoll >= 3_000) {
          const stallShare = Math.min(1, stalls / (now - lastPoll));
          stalls = 0;
          lastPoll = now;
          polling = true;
          void invoke<Omit<ResourceSample, "stallShare">>("resource_usage")
            .then((usage) => {
              if (!alive) return;
              const sample = { ...usage, stallShare };
              state = samplePressure(state, sample, now);
              setPressure((previous) =>
                previous.active === state.active &&
                previous.reasons.join() === state.reasons.join()
                  ? previous
                  : state,
              );
            })
            .catch(() => {})
            .finally(() => {
              polling = false;
            });
        }
      }
      lastTick = now;
      timer = window.setTimeout(tick, 500);
    };
    timer = window.setTimeout(tick, 500);
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, []);

  useEffect(() => {
    if (!open || !chipHost) return;
    let alive = true;
    let timer = 0;
    setLoading(true);
    const refresh = async () => {
      try {
        const rows = await invoke<OwnedProcess[]>("resource_processes");
        if (alive) {
          // Keep controls under the pointer while CPU usage changes between polls.
          setProcesses((previous) => {
            const remaining = new Map(rows.map((row) => [row.identity, row]));
            const ordered: OwnedProcess[] = [];
            for (const old of previous) {
              const row = remaining.get(old.identity);
              if (row) ordered.push(row);
              remaining.delete(old.identity);
            }
            const sessionOrder = new Map(
              [...new Set(previous.map((row) => row.sessionId))].map(
                (id, index) => [id, index],
              ),
            );
            return [...ordered, ...remaining.values()].sort(
              (a, b) =>
                (sessionOrder.get(a.sessionId) ?? sessionOrder.size) -
                (sessionOrder.get(b.sessionId) ?? sessionOrder.size),
            );
          });
          setError("");
        }
      } catch (error) {
        if (alive) setError(String(error));
      } finally {
        if (alive) {
          setLoading(false);
          timer = window.setTimeout(refresh, 3_000);
        }
      }
    };
    void refresh();
    return () => {
      alive = false;
      clearTimeout(timer);
    };
  }, [open, stopping, chipHost]);

  if (!pressure.active && !open) return null;
  const sessionFor = (id: string) =>
    sessions.current.find((session) => session.id === id);
  const titleFor = (id: string) => {
    const session = sessionFor(id);
    return session
      ? sessionDisplayTitle(session.title, session.harness)
      : "Background work";
  };
  const stopWork = async (
    identity: string,
    stop: () => void | Promise<unknown>,
  ) => {
    setStopError("");
    setStopping(identity);
    try {
      await stop();
    } catch (error) {
      setStopError(String(error));
    } finally {
      setStopping(undefined);
    }
  };
  const groups = new Map<string, OwnedProcess[]>();
  for (const process of processes) {
    const group = groups.get(process.sessionId);
    if (group) group.push(process);
    else groups.set(process.sessionId, [process]);
  }
  const reasons = pressure.reasons.map((reason) =>
    reason === "CPU saturation"
      ? "High CPU usage"
      : reason === "Memory pressure"
        ? "Low available memory"
        : "Slow response",
  );
  return (
    <>
      {pressure.active && chipHost
        ? createPortal(
            <button
              type="button"
              aria-label="High resource usage"
              aria-haspopup="dialog"
              aria-expanded={open}
              onClick={() => setOpen(true)}
              title={`${reasons.join(" · ")} · Review running work`}
              className={`inline-flex h-5 shrink-0 items-center gap-1.5 whitespace-nowrap ${insetChip ? "rounded-full px-2" : "rounded-md px-1.5"} text-content/55 transition-colors hover:bg-content/8 hover:text-content focus-visible:outline-2 focus-visible:outline-accent ${open ? "bg-content/8 text-content" : ""}`}
            >
              <CircleAlert
                className="size-3.5 shrink-0 text-amber-500 dark:text-amber-400"
                strokeWidth={1.75}
                aria-hidden
              />
              <span>High usage</span>
            </button>,
            chipHost,
          )
        : null}
      {open && chipHost ? (
        <Modal
          title="Running work"
          size="sm"
          fitViewport
          onClose={() => setOpen(false)}
        >
          <div className="p-4 text-[13px]">
            <div className="flex items-start gap-2.5 rounded-lg bg-content/[0.04] px-3 py-2.5">
              {pressure.active ? (
                <CircleAlert className="mt-0.5 size-4 shrink-0 text-amber-500 dark:text-amber-400" />
              ) : (
                <Check className="mt-0.5 size-4 shrink-0 text-emerald-500" />
              )}
              <div className="min-w-0">
                <p className="text-[12px] font-medium">
                  {pressure.active
                    ? reasons.join(" · ")
                    : "Usage is back to normal"}
                </p>
                <p className="mt-0.5 text-[12px] leading-relaxed text-content/55">
                  {pressure.active
                    ? "Things may feel slower. Stop work you no longer need."
                    : "The slowdown has cleared. You can close this window."}
                </p>
              </div>
            </div>
            {error ? (
              <p
                role="alert"
                title={error}
                className="mt-3 text-[12px] text-red-500 dark:text-red-400"
              >
                Couldn't load running work. Trying again…
              </p>
            ) : null}
            {stopError ? (
              <p
                role="alert"
                title={stopError}
                className="mt-3 text-[12px] text-red-500 dark:text-red-400"
              >
                Couldn't stop this work. Please try again.
              </p>
            ) : null}
            {loading && !processes.length ? (
              <p
                role="status"
                className="flex items-center justify-center gap-2 py-8 text-[12px] text-content/50"
              >
                <Loader className="size-3.5 animate-spin" />
                Checking running work…
              </p>
            ) : !processes.length && !error ? (
              <p className="py-6 text-center text-[12px] leading-relaxed text-content/50">
                No running work found in MonoCode.
                {pressure.active
                  ? " Other apps may be using these resources."
                  : ""}
              </p>
            ) : null}
            {processes.length ? (
              <>
                <p className="mt-4 mb-1 text-[11px] font-medium text-content/45">
                  {groups.size} {groups.size === 1 ? "session" : "sessions"}{" "}
                  running
                </p>
                <div
                  ref={lockOverscroll}
                  role="list"
                  aria-label="Running sessions"
                  className="max-h-[min(320px,45dvh)] divide-y divide-stroke overflow-y-auto overscroll-contain"
                >
                  {[...groups].map(([id, rows]) => {
                    const session = sessionFor(id);
                    const title = titleFor(id);
                    const memory = rows.reduce(
                      (sum, row) => sum + row.memoryBytes,
                      0,
                    );
                    return (
                      <div
                        key={id}
                        role="listitem"
                        data-resource-session={id}
                        className="py-3"
                      >
                        <div className="flex items-center gap-2.5">
                          <span className="grid size-6 shrink-0 place-items-center rounded-md bg-content/5 text-content/50">
                            {session ? (
                              <HarnessIcon harness={session.harness} />
                            ) : (
                              <CircleAlert className="size-3.5" />
                            )}
                          </span>
                          <span
                            className="min-w-0 flex-1 truncate font-medium"
                            title={title}
                          >
                            {title}
                          </span>
                          {session ? (
                            <button
                              type="button"
                              aria-label={`Stop session ${title}`}
                              title="Stop this session and pause queued work"
                              disabled={!!stopping}
                              className="flex h-7 shrink-0 items-center gap-1.5 rounded-md bg-content/6 px-2.5 text-[12px] text-content/70 hover:bg-red-500/10 hover:text-red-500 disabled:opacity-40 dark:hover:text-red-400"
                              onClick={() =>
                                void stopWork(`session:${id}`, () =>
                                  onStopSession(id),
                                )
                              }
                            >
                              {stopping === `session:${id}` ? (
                                <>
                                  <Loader className="size-3 animate-spin" />
                                  Stopping…
                                </>
                              ) : (
                                "Stop"
                              )}
                            </button>
                          ) : null}
                        </div>
                        <details className="group/processes ml-[34px] mt-0.5">
                          <summary className="flex cursor-pointer list-none items-center gap-1 text-[11px] text-content/45 hover:text-content/75 [&::-webkit-details-marker]:hidden">
                            <ChevronDown className="size-3 shrink-0 -rotate-90 transition-transform group-open/processes:rotate-0" />
                            {rows.length}{" "}
                            {rows.length === 1 ? "process" : "processes"} ·{" "}
                            {formatMemory(memory)} memory
                          </summary>
                          <div className="mt-2 space-y-2 border-l border-stroke pl-2.5">
                            {rows.map((process) => (
                              <div
                                key={process.identity}
                                className="flex min-w-0 items-center gap-2"
                              >
                                <div className="min-w-0 flex-1">
                                  <code
                                    className="block truncate text-[11px] text-content/65"
                                    title={process.command}
                                  >
                                    {process.command}
                                  </code>
                                  <p
                                    className="truncate text-[10px] tabular-nums text-content/40"
                                    title={process.cwd}
                                  >
                                    {process.cpuPercent.toFixed(0)}% CPU ·{" "}
                                    {formatMemory(process.memoryBytes)} · PID{" "}
                                    {process.pid}
                                  </p>
                                </div>
                                <button
                                  type="button"
                                  aria-label={`${process.agent ? "Stop agent" : "Stop build/tool"} for ${title} (PID ${process.pid})`}
                                  title={
                                    process.agent
                                      ? "Interrupt the agent"
                                      : "Stop this tool; keep the agent running"
                                  }
                                  disabled={
                                    !!stopping || (process.agent && !session)
                                  }
                                  className="shrink-0 rounded px-1.5 py-1 text-[11px] text-content/50 hover:bg-red-500/10 hover:text-red-500 disabled:opacity-40 dark:hover:text-red-400"
                                  onClick={() =>
                                    void stopWork(process.identity, () =>
                                      process.agent
                                        ? onStopAgent(id)
                                        : invoke("resource_stop_tool", {
                                            pid: process.pid,
                                            identity: process.identity,
                                          }),
                                    )
                                  }
                                >
                                  {stopping === process.identity
                                    ? "Stopping…"
                                    : "Stop"}
                                </button>
                              </div>
                            ))}
                          </div>
                        </details>
                      </div>
                    );
                  })}
                </div>
                <p className="mt-3 border-t border-stroke pt-3 text-[11px] text-content/40">
                  Stopping work keeps your conversations saved.
                </p>
              </>
            ) : null}
          </div>
        </Modal>
      ) : null}
    </>
  );
}

function formatMemory(bytes: number): string {
  const mb = bytes / 1024 / 1024;
  return mb >= 1024
    ? `${(mb / 1024).toFixed(1)} GB`
    : mb < 1
      ? "<1 MB"
      : `${Math.round(mb)} MB`;
}
