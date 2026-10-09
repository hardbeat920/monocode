import { useEffect, useId, useRef, useState } from "react";
import { ChevronRight } from "../../../shared/ui/icons";
import { MonoWorkTicker } from "./MonoWorkTicker";

/** Liveness observers are not individual agents; never turn their count into progress. */
export function BackgroundActivity({
  tasks,
  busy,
  visible,
  interrupted = false,
  onShowWork,
}: {
  tasks?: string[];
  busy: boolean;
  visible: boolean;
  interrupted?: boolean;
  onShowWork?: () => void;
}) {
  const waiting = busy && !!tasks?.length;
  const [lastTasks, setLastTasks] = useState<string[]>([]);
  const [startedAt, setStartedAt] = useState<number>();
  const [elapsed, setElapsed] = useState(0);
  const [open, setOpen] = useState(true);
  const detailsId = useId();
  const wasWaiting = useRef(false);

  useEffect(() => {
    if (waiting) {
      setLastTasks(tasks!);
      if (!wasWaiting.current) {
        setStartedAt(Date.now());
        setElapsed(0);
        setOpen(true);
      }
    }
    wasWaiting.current = waiting;
  }, [waiting, tasks]);

  useEffect(() => {
    if (!busy || !visible || startedAt == null) return;
    const tick = () =>
      setElapsed(Math.max(0, Math.floor((Date.now() - startedAt) / 1000)));
    tick();
    const timer = window.setInterval(tick, 1000);
    return () => window.clearInterval(timer);
  }, [busy, visible, startedAt]);

  if (!waiting && !lastTasks.length) return null;
  const phase = interrupted
    ? "stopped"
    : waiting
      ? "waiting"
      : busy
        ? "continuing"
        : "ended";
  const label = {
    waiting: "Background work is running",
    continuing: "Continuing response",
    ended: "Background activity ended",
    stopped: "Turn stopped",
  }[phase];
  const expanded = open && waiting && !interrupted;
  const descriptions = [
    ...new Set(
      (tasks?.length ? tasks : lastTasks).map((task) =>
        task === "pi-subagents" ? "Delegated work" : task,
      ),
    ),
  ];

  return (
    <section
      className="background-activity"
      data-phase={phase}
      data-live={visible && busy && !interrupted}
      aria-label="Background activity"
    >
      <div className="background-activity-heading">
        <span className="background-activity-mark" aria-hidden="true">
          <svg viewBox="0 0 20 20" fill="none">
            <circle
              cx="10"
              cy="10"
              r="7"
              stroke="currentColor"
              opacity=".16"
              strokeWidth="1.5"
            />
            <circle
              className="background-activity-orbit"
              cx="10"
              cy="10"
              r="7"
              stroke="currentColor"
              strokeWidth="1.5"
              strokeLinecap="round"
              strokeDasharray="13 31"
            />
          </svg>
        </span>
        <span role="status" className="sr-only">
          {label}
        </span>
        <div className="min-w-0 flex-1" aria-hidden="true">
          <MonoWorkTicker
            status={{ key: phase, kind: "agent", label, active: false }}
            showIcon={false}
          />
        </div>
        <span
          className="background-activity-time"
          title="Elapsed since background activity appeared"
        >
          {Math.floor(elapsed / 60)}:{String(elapsed % 60).padStart(2, "0")}
        </span>
        {waiting && !interrupted ? (
          <button
            type="button"
            className="background-activity-toggle"
            aria-label={
              expanded ? "Hide background activity" : "Show background activity"
            }
            aria-expanded={expanded}
            aria-controls={detailsId}
            onClick={() => setOpen(!open)}
          >
            <ChevronRight className="size-3.5" />
          </button>
        ) : (
          <span className="size-6 shrink-0" aria-hidden="true" />
        )}
      </div>
      <div
        id={detailsId}
        className="background-activity-disclosure"
        data-open={expanded}
        inert={!expanded}
      >
        <div className="background-activity-clip">
          <div className="background-activity-body">
            <ul className="background-activity-tasks">
              {descriptions.map((task) => (
                <li key={task}>
                  <span aria-hidden="true" />
                  {task}
                </li>
              ))}
            </ul>
            <div className="background-activity-footer">
              <p>The response will continue automatically.</p>
              {onShowWork ? (
                <button type="button" onClick={onShowWork}>
                  View activity
                  <ChevronRight className="size-3" aria-hidden="true" />
                </button>
              ) : null}
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}
