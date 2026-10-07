import { useEffect, useState, type ReactNode } from "react";
import { Clock, Pause, Play, Trash2, Zap } from "../../../shared/ui/icons";
import { mascotPath } from "../../projects/model/projectMascots";
import type { MonoState } from "../model/mono";
import { subscribeMonoFiles } from "../model/monoFiles";
import {
  checkHabitsNow,
  habitScheduleLabel,
  habitsToday,
  habitStarting,
  loadHabits,
  nextHabitRunAt,
  subscribeHabitsRunning,
  updateHabits,
  type Habit,
} from "../model/monoHabits";
import { RunningFor, useHabitRunning } from "./HabitPage";
import { Empty, Section } from "./monoPanelParts";

/** The Mono's habits, reloaded whenever its folder changes or a run ends. */
export function useHabits(
  monoId: string,
  status: MonoState["status"],
): Habit[] | undefined {
  const [habits, setHabits] = useState<Habit[]>();
  const working = status === "working";
  useEffect(() => {
    let live = true;
    const refresh = () => {
      void loadHabits(monoId)
        .then((next) => {
          if (live) setHabits(next);
        })
        .catch(() => {});
    };
    refresh();
    const stop = subscribeMonoFiles(refresh);
    window.addEventListener("focus", refresh);
    // Runs move nextRunAt along without a panel event; check now and then.
    const timer = window.setInterval(refresh, 30_000);
    return () => {
      live = false;
      stop();
      window.removeEventListener("focus", refresh);
      window.clearInterval(timer);
    };
  }, [monoId, working]);
  return habits;
}

export type HabitActions = {
  runNow(id: string): void;
  toggle(id: string): void;
  remove(id: string): void;
};

/** Edits to the Mono's habits from the panel, saved over what was read. */
export function habitActions(monoId: string): HabitActions {
  const change = (
    id: string,
    edit: (habit: Habit) => Habit | null,
    then?: () => void,
  ) =>
    void updateHabits(monoId, (list) => ({
      habits: list.flatMap((habit) => {
        if (habit.id !== id) return [habit];
        const next = edit(habit);
        return next ? [next] : [];
      }),
      result: undefined,
    }))
      .then(then)
      .catch((error) => console.warn("Could not change the habit", error));
  return {
    runNow: (id) =>
      change(id, (habit) => ({ ...habit, runRequested: true }), checkHabitsNow),
    toggle: (id) =>
      change(id, (habit) =>
        habit.enabled
          ? { ...habit, enabled: false }
          : {
              ...habit,
              enabled: true,
              nextRunAt: nextHabitRunAt(habit.schedule, Date.now()),
            },
      ),
    remove: (id) => change(id, () => null),
  };
}

export function HabitRow({
  habit,
  actions,
  onOpen,
}: {
  habit: Habit;
  actions: HabitActions;
  onOpen: () => void;
}) {
  const runningSince = useHabitRunning(habit.id);
  const running = runningSince != null;
  const starting = !running && habitStarting(habit);
  return (
    <li
      data-habit={habit.id}
      data-habit-running={running || undefined}
      className="group/habit flex items-center rounded-md hover:bg-content/5"
    >
      <button
        type="button"
        onClick={onOpen}
        className="flex min-w-0 flex-1 items-center gap-2.5 rounded-md px-2 py-2 text-left"
      >
        <span
          aria-hidden
          className={`grid size-7 shrink-0 place-items-center rounded-md bg-content/6 ${
            habit.enabled || running ? "text-content/60" : "text-content/30"
          }`}
        >
          {running ? (
            <span className="size-2 animate-pulse rounded-full bg-[var(--mono-color)]" />
          ) : (
            <Clock className="size-3.5" strokeWidth={1.75} />
          )}
        </span>
        <span className="flex min-w-0 flex-1 flex-col">
          <span
            className={`truncate text-[12px] leading-5 ${
              habit.enabled || running ? "text-content/85" : "text-content/45"
            }`}
          >
            {habit.name}
          </span>
          <span className="truncate text-[11px] leading-4 text-content/40">
            {running ? (
              <>
                Running now · <RunningFor since={runningSince} />
              </>
            ) : starting ? (
              "Starting…"
            ) : habit.enabled ? (
              habitScheduleLabel(habit.schedule)
            ) : (
              `Paused · ${habitScheduleLabel(habit.schedule)}`
            )}
          </span>
        </span>
      </button>
      <span className="flex shrink-0 items-center pr-2 opacity-0 transition-opacity group-hover/habit:opacity-100 focus-within:opacity-100">
        <HabitButton
          label={
            running ? "Already running" : starting ? "Starting" : "Run now"
          }
          disabled={running || starting}
          onClick={() => actions.runNow(habit.id)}
        >
          <Zap className="size-3.5" strokeWidth={1.75} />
        </HabitButton>
        <HabitButton
          label={habit.enabled ? "Pause" : "Resume"}
          onClick={() => actions.toggle(habit.id)}
        >
          {habit.enabled ? (
            <Pause className="size-3.5" strokeWidth={1.75} />
          ) : (
            <Play className="size-3.5" strokeWidth={1.75} />
          )}
        </HabitButton>
        <HabitButton label="Remove" onClick={() => actions.remove(habit.id)}>
          <Trash2 className="size-3.5" strokeWidth={1.75} />
        </HabitButton>
      </span>
    </li>
  );
}

export function HabitButton({
  label,
  disabled = false,
  onClick,
  children,
}: {
  label: string;
  disabled?: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      disabled={disabled}
      onClick={onClick}
      className="grid size-6 place-items-center rounded text-content/50 enabled:hover:bg-content/8 enabled:hover:text-content disabled:opacity-35"
    >
      {children}
    </button>
  );
}

export function HabitsList({
  habits,
  actions,
  onOpen,
}: {
  habits: Habit[] | undefined;
  actions: HabitActions;
  onOpen: (id: string) => void;
}) {
  if (!habits) return <Empty>Loading…</Empty>;
  if (habits.length === 0) return <Empty>No habits yet</Empty>;
  return (
    <ul className="flex flex-col gap-px">
      {habits.map((habit) => (
        <HabitRow
          key={habit.id}
          habit={habit}
          actions={actions}
          onOpen={() => onOpen(habit.id)}
        />
      ))}
    </ul>
  );
}

/** Re-renders when a run starts or ends, and each minute for the day. */
function useTodayTick(): number {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const tick = () => setNow(Date.now());
    const timer = window.setInterval(tick, 60_000);
    const stop = subscribeHabitsRunning(tick);
    return () => {
      window.clearInterval(timer);
      stop();
    };
  }, []);
  return now;
}

/**
 * Today's habits on the panel's front: what is still to come, then what has
 * already run. A Mono without habits shows the empty clock instead.
 */
export function TodayHabits({
  habits,
  actions,
  onOpen,
}: {
  habits: Habit[] | undefined;
  actions: HabitActions;
  onOpen: (id: string) => void;
}) {
  const now = useTodayTick();
  if (!habits) return null;
  if (habits.length === 0) return <HabitsEmpty />;
  const today = habitsToday(habits, now);
  if (today.length === 0) return <div className="border-t border-stroke" />;
  const upcoming = today.filter((item) => item.state !== "done");
  const done = today.filter((item) => item.state === "done");
  const cards = (items: typeof today) => (
    <ul className="flex flex-col gap-px">
      {items.map(({ habit }) => (
        <HabitRow
          key={habit.id}
          habit={habit}
          actions={actions}
          onOpen={() => onOpen(habit.id)}
        />
      ))}
    </ul>
  );
  return (
    <>
      {upcoming.length ? (
        <Section title="Upcoming">{cards(upcoming)}</Section>
      ) : null}
      {done.length ? <Section title="Done">{cards(done)}</Section> : null}
    </>
  );
}

/**
 * A pixel alarm clock on the mascots' grid, the habits' counterpart to the
 * empty session list's terminal. The glow scatters around the bells and feet.
 */
const CLOCK = [
  "..##........##..",
  ".###..####..###.",
  ".#..##....##..#.",
  "...#....#...#...",
  "..#.....#....#..",
  "..#.....#....#..",
  "..#.....###..#..",
  "..#..........#..",
  "...#........#...",
  "....##....##....",
  "......####......",
  "...##......##...",
];

const CLOCK_GLOW = [
  "#..............#",
  "................",
  "#..............#",
  "................",
  "#..............#",
  "................",
  "...............#",
  "#...............",
  "................",
  "................",
  "..#..........#..",
  "#..............#",
];

const CLOCK_PATH = mascotPath(CLOCK);
const CLOCK_GLOW_PATH = mascotPath(CLOCK_GLOW);

/** Empty state for a Mono with no habits yet. */
export function HabitsEmpty() {
  return (
    <div className="flex flex-col items-center justify-center gap-4 border-t border-stroke px-6 py-10 text-center">
      <svg
        aria-hidden
        viewBox={`0 0 16 ${CLOCK.length}`}
        shapeRendering="crispEdges"
        className="w-20 text-content/25"
        fill="currentColor"
      >
        <path d={CLOCK_GLOW_PATH} opacity={0.4} />
        <path d={CLOCK_PATH} />
      </svg>
      <p className="text-[12px] text-content/45">No habits yet</p>
    </div>
  );
}
