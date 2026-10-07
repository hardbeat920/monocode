import {
  useEffect,
  useState,
  useSyncExternalStore,
  type CSSProperties,
  type ReactNode,
} from "react";
import { Pause, Play, Trash2, Zap } from "../../../shared/ui/icons";
import { IconButton } from "../../../app/shell/TitleBar";
import { PageHeader } from "./monoPanelParts";
import { AgentMarkdown } from "../../sessions/ui/AgentMarkdown";
import {
  habitRunningSince,
  habitScheduleLabel,
  habitStarting,
  subscribeHabitsRunning,
  type Habit,
  type HabitRun,
} from "../model/monoHabits";

const dayTime = new Intl.DateTimeFormat(undefined, {
  weekday: "short",
  hour: "numeric",
  minute: "2-digit",
});
const dateTime = new Intl.DateTimeFormat(undefined, {
  month: "short",
  day: "numeric",
  hour: "numeric",
  minute: "2-digit",
});

export function when(at: number, now = Date.now()): string {
  const date = new Date(at);
  const today = new Date(now);
  if (date.toDateString() === today.toDateString())
    return `Today ${new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" }).format(at)}`;
  return now - at < 6 * 24 * 60 * 60 * 1000
    ? dayTime.format(at)
    : dateTime.format(at);
}

function duration(ms: number): string {
  const seconds = Math.max(1, Math.round(ms / 1000));
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  const rest = seconds % 60;
  return rest ? `${minutes}m ${rest}s` : `${minutes}m`;
}

const OUTCOME: Record<HabitRun["outcome"], { label: string; dot: string }> = {
  posted: { label: "Messaged you", dot: "bg-[var(--mono-color)]" },
  quiet: { label: "Nothing to report", dot: "bg-content/25" },
  failed: { label: "Couldn't finish", dot: "bg-red-500/70" },
};

/** One habit up close: what it does, when, and how its recent runs went. */
export function HabitPage({
  habit,
  color,
  cwd,
  onBack,
  onRunNow,
  onToggle,
  onRemove,
}: {
  habit: Habit;
  /** The Mono's color, for runs that reached the user. */
  color: string;
  cwd: string;
  onBack: () => void;
  onRunNow: () => void;
  onToggle: () => void;
  onRemove: () => void;
}) {
  const runningSince = useHabitRunning(habit.id);
  const starting = runningSince == null && habitStarting(habit);
  const runs = habit.runs ?? [];
  return (
    <div
      className="flex min-h-0 flex-1 flex-col"
      style={{ "--mono-color": color } as CSSProperties}
      data-habit-page={habit.id}
    >
      <PageHeader title={habit.name} onBack={onBack}>
        <IconButton
          label={
            runningSince != null
              ? "Already running"
              : starting
                ? "Starting"
                : "Run now"
          }
          disabled={runningSince != null || starting}
          onClick={onRunNow}
        >
          <Zap className="size-3.5" strokeWidth={1.75} />
        </IconButton>
        <IconButton
          label={habit.enabled ? "Pause" : "Resume"}
          onClick={onToggle}
        >
          {habit.enabled ? (
            <Pause className="size-3.5" strokeWidth={1.75} />
          ) : (
            <Play className="size-3.5" strokeWidth={1.75} />
          )}
        </IconButton>
        <IconButton label="Remove" onClick={onRemove}>
          <Trash2 className="size-3.5" strokeWidth={1.75} />
        </IconButton>
      </PageHeader>

      <div className="min-h-0 flex-1 overflow-y-auto overscroll-none">
        <dl className="flex flex-col gap-0.5 px-4 py-3 text-[12px]">
          <Row label="Runs">
            {habit.enabled ? habitScheduleLabel(habit.schedule) : "Paused"}
          </Row>
          {starting ? (
            <Row label="Next">Starting…</Row>
          ) : habit.enabled ? (
            <Row label="Next">{when(habit.nextRunAt)}</Row>
          ) : null}
        </dl>

        <Section title="What it does">
          <p className="whitespace-pre-wrap px-2 text-[12px] leading-5 text-content/75">
            {habit.instructions}
          </p>
        </Section>

        <Section title="Recent runs">
          {runningSince != null ? <RunningRow since={runningSince} /> : null}
          {runs.length === 0 && runningSince == null ? (
            <p className="px-2 py-3 text-[12px] text-content/40">
              It hasn't run yet.
            </p>
          ) : (
            <ul className="flex flex-col gap-px">
              {runs.map((run) => (
                <RunRow key={run.at} run={run} cwd={cwd} />
              ))}
            </ul>
          )}
        </Section>
      </div>
    </div>
  );
}

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="grid grid-cols-[5.5rem_minmax(0,1fr)] items-center">
      <dt className="flex h-7 items-center text-content/45">{label}</dt>
      <dd className="min-w-0 truncate text-content/85">{children}</dd>
    </div>
  );
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="border-t border-stroke px-2 pb-4 pt-3">
      <h4 className="px-2 pb-2 text-[11px] font-medium uppercase tracking-wide text-content/40">
        {title}
      </h4>
      {children}
    </section>
  );
}

/** When the habit's current run started, or undefined while it is idle. */
export function useHabitRunning(habitId: string): number | undefined {
  return useSyncExternalStore(subscribeHabitsRunning, () =>
    habitRunningSince(habitId),
  );
}

/** How long it has been running, ticking each second. */
export function RunningFor({ since }: { since: number }) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(timer);
  }, []);
  return <span className="tabular-nums">{duration(now - since)}</span>;
}

function RunningRow({ since }: { since: number }) {
  return (
    <div className="flex items-center gap-2.5 rounded-md px-2 py-2 text-[12px]">
      <span className="size-2 shrink-0 animate-pulse rounded-full bg-[var(--mono-color)]" />
      <span className="flex-1 text-content/85">Running now</span>
      <span className="text-content/40">
        <RunningFor since={since} />
      </span>
    </div>
  );
}

function RunRow({ run, cwd }: { run: HabitRun; cwd: string }) {
  const [open, setOpen] = useState(false);
  const detail = run.report ?? run.error;
  const outcome = OUTCOME[run.outcome];
  const expanded = open && !!detail;
  return (
    // An open run is one card: the row and its report share a background,
    // and the report starts where the outcome text does.
    <li className={`rounded-md ${expanded ? "bg-content/5" : ""}`}>
      <button
        type="button"
        disabled={!detail}
        aria-expanded={detail ? open : undefined}
        onClick={() => setOpen((value) => !value)}
        className={`flex w-full items-center gap-2.5 rounded-md px-2 py-2 text-left text-[12px] ${
          expanded ? "" : "enabled:hover:bg-content/5"
        }`}
      >
        <span className={`size-2 shrink-0 rounded-full ${outcome.dot}`} />
        <span className="min-w-0 flex-1 truncate text-content/85">
          {outcome.label}
        </span>
        <span className="shrink-0 tabular-nums text-content/40">
          {when(run.at)}
          {run.durationMs ? ` · ${duration(run.durationMs)}` : ""}
        </span>
      </button>
      {expanded ? (
        // 8px row padding + 8px dot + 10px gap: the outcome text's left edge.
        <div className="pb-3 pl-[26px] pr-3 text-[12px] leading-5 text-content/75">
          {run.report ? (
            <AgentMarkdown
              className="agent-chat-bubble-md mono-run-report"
              text={run.report}
              streaming={false}
              cwd={cwd}
            />
          ) : (
            <p className="whitespace-pre-wrap text-red-500/80">{run.error}</p>
          )}
        </div>
      ) : null}
    </li>
  );
}
