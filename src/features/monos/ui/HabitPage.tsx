import {
  useEffect,
  useState,
  useSyncExternalStore,
  type CSSProperties,
  type ReactNode,
} from "react";
import { getLocaleTag, t as translate } from "../../../shared/i18n/locale";
import { useT } from "../../../shared/i18n/useT";
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

export function when(at: number, now = Date.now()): string {
  const date = new Date(at);
  const today = new Date(now);
  const locale = getLocaleTag();
  if (date.toDateString() === today.toDateString())
    return translate("monos.habit.today", {
      time: new Intl.DateTimeFormat(locale, {
        hour: "numeric",
        minute: "2-digit",
      }).format(at),
    });
  return now - at < 6 * 24 * 60 * 60 * 1000
    ? new Intl.DateTimeFormat(locale, {
        weekday: "short",
        hour: "numeric",
        minute: "2-digit",
      }).format(at)
    : new Intl.DateTimeFormat(locale, {
        month: "short",
        day: "numeric",
        hour: "numeric",
        minute: "2-digit",
      }).format(at);
}

function duration(ms: number): string {
  const seconds = Math.max(1, Math.round(ms / 1000));
  if (seconds < 60) return translate("monos.habit.seconds", { seconds });
  const minutes = Math.floor(seconds / 60);
  const rest = seconds % 60;
  return rest
    ? translate("monos.habit.minutesSeconds", { minutes, seconds: rest })
    : translate("monos.habit.minutes", { minutes });
}

const OUTCOME: Record<
  HabitRun["outcome"],
  { label: () => string; dot: string }
> = {
  posted: {
    label: () => translate("monos.habit.posted"),
    dot: "bg-[var(--mono-color)]",
  },
  quiet: {
    label: () => translate("monos.habit.quiet"),
    dot: "bg-content/25",
  },
  failed: {
    label: () => translate("monos.habit.failed"),
    dot: "bg-red-500/70",
  },
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
  const t = useT();
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
              ? t("monos.habit.alreadyRunning")
              : starting
                ? t("monos.habit.starting")
                : t("monos.habit.runNow")
          }
          disabled={runningSince != null || starting}
          onClick={onRunNow}
        >
          <Zap className="size-3.5" strokeWidth={1.75} />
        </IconButton>
        <IconButton
          label={
            habit.enabled ? t("monos.habit.pause") : t("monos.habit.resume")
          }
          onClick={onToggle}
        >
          {habit.enabled ? (
            <Pause className="size-3.5" strokeWidth={1.75} />
          ) : (
            <Play className="size-3.5" strokeWidth={1.75} />
          )}
        </IconButton>
        <IconButton label={t("monos.habit.remove")} onClick={onRemove}>
          <Trash2 className="size-3.5" strokeWidth={1.75} />
        </IconButton>
      </PageHeader>

      <div className="min-h-0 flex-1 overflow-y-auto overscroll-none">
        <dl className="flex flex-col gap-0.5 px-4 py-3 text-[12px]">
          <Row label={t("monos.habit.runs")}>
            {habit.enabled
              ? habitScheduleLabel(habit.schedule)
              : t("monos.habit.paused")}
          </Row>
          {starting ? (
            <Row label={t("monos.habit.next")}>
              {t("monos.habit.startingEllipsis")}
            </Row>
          ) : habit.enabled ? (
            <Row label={t("monos.habit.next")}>{when(habit.nextRunAt)}</Row>
          ) : null}
        </dl>

        <Section title={t("monos.habit.whatItDoes")}>
          <p className="whitespace-pre-wrap px-2 text-[12px] leading-5 text-content/75">
            {habit.instructions}
          </p>
        </Section>

        <Section title={t("monos.habit.recentRuns")}>
          {runningSince != null ? <RunningRow since={runningSince} /> : null}
          {runs.length === 0 && runningSince == null ? (
            <p className="px-2 py-3 text-[12px] text-content/40">
              {t("monos.habit.noRuns")}
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
  const t = useT();
  return (
    <div className="flex items-center gap-2.5 rounded-md px-2 py-2 text-[12px]">
      <span className="size-2 shrink-0 animate-pulse rounded-full bg-[var(--mono-color)]" />
      <span className="flex-1 text-content/85">
        {t("monos.habit.runningNow")}
      </span>
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
          {outcome.label()}
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
