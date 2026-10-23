import { useEffect, useRef, useState } from "react";
import { isImeComposition } from "../../../shared/lib/keyboard";
import { SearchableSelect } from "../../../shared/ui/SearchableSelect";
import { AUTOMATION_WEEKDAYS } from "../../automations/model/automations";
import {
  addHabit,
  editedHabit,
  editHabit,
  nextHabitRunAt,
  type Habit,
  type HabitSchedule,
} from "../model/monoHabits";
import { when } from "./HabitPage";
import { AutoTextarea, PageHeader, Property, Section } from "./monoPanelParts";

const KINDS = [
  { value: "daily", label: "Daily" },
  { value: "weekdays", label: "Weekdays" },
  { value: "weekly", label: "Weekly" },
  { value: "hourly", label: "Hourly" },
] as const;

const DAYS = AUTOMATION_WEEKDAYS.map((label, value) => ({
  value: String(value),
  label,
}));

const MINUTES = [0, 15, 30, 45].map((value) => ({
  value: String(value),
  label: `:${String(value).padStart(2, "0")}`,
}));

/**
 * A habit written by hand rather than set up in the chat, or one being
 * edited, laid out like the habit's own page: its name, when it runs, and
 * what it does.
 */
export function HabitFormPage({
  monoId,
  habit,
  onBack,
  onSaved,
}: {
  monoId: string;
  /** The habit to edit; a new one when left out. */
  habit?: Habit;
  onBack: () => void;
  onSaved: () => void;
}) {
  const [name, setName] = useState(habit?.name ?? "");
  const [instructions, setInstructions] = useState(habit?.instructions ?? "");
  const [schedule, setSchedule] = useState<HabitSchedule>(
    habit?.schedule ?? {
      scheduleKind: "daily",
      time: "09:00",
      minute: 0,
      dayOfWeek: 1,
    },
  );
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string>();
  const nameRef = useRef<HTMLInputElement>(null);
  // The page mounts offscreen to slide in; autoFocus would scroll the panel
  // over to it first, so the slide would jump.
  useEffect(() => nameRef.current?.focus({ preventScroll: true }), []);
  const ready = !!name.trim() && !!instructions.trim() && !saving;

  const save = () => {
    if (!ready) return;
    setSaving(true);
    setError(undefined);
    const draft = {
      name: name.trim().slice(0, 80),
      instructions: instructions.trim().slice(0, 4_000),
      schedule,
    };
    (habit ? editHabit(monoId, habit.id, draft) : addHabit(monoId, draft))
      .then(onSaved)
      .catch((reason: unknown) => {
        setSaving(false);
        setError(
          reason instanceof Error
            ? reason.message
            : habit
              ? "Could not save the habit."
              : "Could not add the habit.",
        );
      });
  };

  return (
    <div
      className="flex min-h-0 flex-1 flex-col"
      {...(habit ? { "data-edit-habit": habit.id } : { "data-new-habit": "" })}
    >
      <PageHeader title={habit ? "Edit habit" : "New habit"} onBack={onBack}>
        <button
          type="button"
          disabled={!ready}
          onClick={save}
          data-tauri-drag-region="false"
          className="h-6 rounded-md bg-content/10 px-2 text-[12px] text-content enabled:hover:bg-content/[0.14] disabled:opacity-40"
        >
          {habit ? "Save" : "Create"}
        </button>
      </PageHeader>

      <div className="min-h-0 flex-1 overflow-y-auto overscroll-none">
        <div className="px-2 pt-3">
          <input
            ref={nameRef}
            aria-label="Name"
            value={name}
            maxLength={80}
            placeholder="Name"
            onChange={(event) => setName(event.target.value)}
            onKeyDown={(event) => {
              if (
                event.key === "Enter" &&
                !isImeComposition(event.nativeEvent)
              ) {
                save();
              }
            }}
            className="h-8 w-full rounded-md bg-transparent px-2 text-[13px] font-medium text-content outline-none placeholder:text-content/35 hover:bg-content/5 focus:bg-content/5"
          />
        </div>

        <dl className="flex flex-col gap-0.5 px-4 py-3">
          <Property label="Runs">
            <ScheduleFields schedule={schedule} onChange={setSchedule} />
          </Property>
          <Property label="Next">
            {habit && !habit.enabled
              ? "Paused"
              : when(
                  habit
                    ? editedHabit(
                        habit,
                        { name, instructions, schedule },
                        Date.now(),
                      ).nextRunAt
                    : nextHabitRunAt(schedule, Date.now()),
                )}
          </Property>
        </dl>

        <Section title="What it does">
          <Instructions
            value={instructions}
            onChange={setInstructions}
            onSubmit={save}
          />
          {error ? (
            <p className="px-2 pt-2 text-[12px] text-red-500/80">{error}</p>
          ) : null}
        </Section>
      </div>
    </div>
  );
}

function ScheduleFields({
  schedule,
  onChange,
}: {
  schedule: HabitSchedule;
  onChange: (schedule: HabitSchedule) => void;
}) {
  const kind = schedule.scheduleKind;
  return (
    <span className="flex flex-wrap items-center gap-1.5 py-0.5">
      <SearchableSelect
        label="Repeats"
        value={kind}
        options={KINDS}
        variant="pill"
        searchable={false}
        onChange={(value) =>
          onChange({
            ...schedule,
            scheduleKind: value as HabitSchedule["scheduleKind"],
          })
        }
      />
      {kind === "weekly" ? (
        <>
          <span className="text-content/45">on</span>
          <SearchableSelect
            label="Day"
            value={String(schedule.dayOfWeek)}
            options={DAYS}
            variant="pill"
            searchable={false}
            onChange={(value) =>
              onChange({ ...schedule, dayOfWeek: Number(value) })
            }
          />
        </>
      ) : null}
      <span className="text-content/45">at</span>
      {kind === "hourly" ? (
        <SearchableSelect
          label="Minute"
          value={String(schedule.minute)}
          options={MINUTES}
          variant="pill"
          searchable={false}
          onChange={(value) => onChange({ ...schedule, minute: Number(value) })}
        />
      ) : (
        <input
          type="time"
          aria-label="Time"
          value={schedule.time}
          required
          onChange={(event) => {
            if (event.target.value)
              onChange({ ...schedule, time: event.target.value });
          }}
          className="h-7 rounded-md bg-content/10 px-2 text-[12px] text-content outline-none hover:bg-content/[0.14] focus-visible:bg-content/[0.14]"
        />
      )}
    </span>
  );
}

/** The habit's instructions, growing with what is typed. ⌘↵ saves it. */
function Instructions({
  value,
  onChange,
  onSubmit,
}: {
  value: string;
  onChange: (value: string) => void;
  onSubmit: () => void;
}) {
  return (
    <AutoTextarea
      aria-label="What it does"
      value={value}
      rows={3}
      maxLength={4_000}
      placeholder="What it should do on each run, and when it's worth telling you about."
      onChange={(event) => onChange(event.target.value)}
      onKeyDown={(event) => {
        if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
          event.preventDefault();
          onSubmit();
        }
      }}
      className="block w-full resize-none rounded-md bg-transparent px-2 py-1 text-[12px] leading-5 text-content/75 outline-none placeholder:text-content/35 hover:bg-content/5 focus:bg-content/5 focus:text-content/90"
    />
  );
}
