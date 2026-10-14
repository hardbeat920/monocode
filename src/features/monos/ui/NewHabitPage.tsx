import { useEffect, useRef, useState } from "react";
import { getLocaleTag, t as translate } from "../../../shared/i18n/locale";
import { useT } from "../../../shared/i18n/useT";
import { isImeComposition } from "../../../shared/lib/keyboard";
import { SearchableSelect } from "../../../shared/ui/SearchableSelect";
import {
  addHabit,
  nextHabitRunAt,
  type HabitSchedule,
} from "../model/monoHabits";
import { when } from "./HabitPage";
import { AutoTextarea, PageHeader, Property, Section } from "./monoPanelParts";

const kinds = () =>
  [
    { value: "daily", label: translate("monos.newHabit.daily") },
    { value: "weekdays", label: translate("monos.newHabit.weekdays") },
    { value: "weekly", label: translate("monos.newHabit.weekly") },
    { value: "hourly", label: translate("monos.newHabit.hourly") },
  ] as const;

/** Sunday first, as `dayOfWeek` counts; 2023-01-01 was a Sunday. */
const days = () => {
  const format = new Intl.DateTimeFormat(getLocaleTag(), {
    weekday: "long",
    timeZone: "UTC",
  });
  return Array.from({ length: 7 }, (_, value) => ({
    value: String(value),
    label: format.format(Date.UTC(2023, 0, 1 + value)),
  }));
};

const MINUTES = [0, 15, 30, 45].map((value) => ({
  value: String(value),
  label: `:${String(value).padStart(2, "0")}`,
}));

/**
 * A habit written by hand rather than set up in the chat, laid out like the
 * habit's own page: its name, when it runs, and what it does.
 */
export function NewHabitPage({
  monoId,
  onBack,
  onCreated,
}: {
  monoId: string;
  onBack: () => void;
  onCreated: () => void;
}) {
  const t = useT();
  const [name, setName] = useState("");
  const [instructions, setInstructions] = useState("");
  const [schedule, setSchedule] = useState<HabitSchedule>({
    scheduleKind: "daily",
    time: "09:00",
    minute: 0,
    dayOfWeek: 1,
  });
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string>();
  const nameRef = useRef<HTMLInputElement>(null);
  // The page mounts offscreen to slide in; autoFocus would scroll the panel
  // over to it first, so the slide would jump.
  useEffect(() => nameRef.current?.focus({ preventScroll: true }), []);
  const ready = !!name.trim() && !!instructions.trim() && !saving;

  const create = () => {
    if (!ready) return;
    setSaving(true);
    setError(undefined);
    addHabit(monoId, {
      name: name.trim().slice(0, 80),
      instructions: instructions.trim().slice(0, 4_000),
      schedule,
    })
      .then(onCreated)
      .catch((reason: unknown) => {
        setSaving(false);
        setError(
          reason instanceof Error
            ? reason.message
            : t("monos.newHabit.addFailed"),
        );
      });
  };

  return (
    <div className="flex min-h-0 flex-1 flex-col" data-new-habit>
      <PageHeader title={t("monos.newHabit.title")} onBack={onBack}>
        <button
          type="button"
          disabled={!ready}
          onClick={create}
          data-tauri-drag-region="false"
          className="h-6 rounded-md bg-content/10 px-2 text-[12px] text-content enabled:hover:bg-content/[0.14] disabled:opacity-40"
        >
          {t("monos.newHabit.create")}
        </button>
      </PageHeader>

      <div className="min-h-0 flex-1 overflow-y-auto overscroll-none">
        <div className="px-2 pt-3">
          <input
            ref={nameRef}
            aria-label={t("monos.newHabit.name")}
            value={name}
            maxLength={80}
            placeholder={t("monos.newHabit.name")}
            onChange={(event) => setName(event.target.value)}
            onKeyDown={(event) => {
              if (
                event.key === "Enter" &&
                !isImeComposition(event.nativeEvent)
              ) {
                create();
              }
            }}
            className="h-8 w-full rounded-md bg-transparent px-2 text-[13px] font-medium text-content outline-none placeholder:text-content/35 hover:bg-content/5 focus:bg-content/5"
          />
        </div>

        <dl className="flex flex-col gap-0.5 px-4 py-3">
          <Property label={t("monos.habit.runs")}>
            <ScheduleFields schedule={schedule} onChange={setSchedule} />
          </Property>
          <Property label={t("monos.habit.next")}>
            {when(nextHabitRunAt(schedule, Date.now()))}
          </Property>
        </dl>

        <Section title={t("monos.habit.whatItDoes")}>
          <Instructions
            value={instructions}
            onChange={setInstructions}
            onSubmit={create}
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
  const t = useT();
  const kind = schedule.scheduleKind;
  return (
    <span className="flex flex-wrap items-center gap-1.5 py-0.5">
      <SearchableSelect
        label={t("monos.newHabit.repeats")}
        value={kind}
        options={kinds()}
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
          <span className="text-content/45">{t("monos.newHabit.on")}</span>
          <SearchableSelect
            label={t("monos.newHabit.day")}
            value={String(schedule.dayOfWeek)}
            options={days()}
            variant="pill"
            searchable={false}
            onChange={(value) =>
              onChange({ ...schedule, dayOfWeek: Number(value) })
            }
          />
        </>
      ) : null}
      <span className="text-content/45">{t("monos.newHabit.at")}</span>
      {kind === "hourly" ? (
        <SearchableSelect
          label={t("monos.newHabit.minute")}
          value={String(schedule.minute)}
          options={MINUTES}
          variant="pill"
          searchable={false}
          onChange={(value) => onChange({ ...schedule, minute: Number(value) })}
        />
      ) : (
        <input
          type="time"
          aria-label={t("monos.newHabit.time")}
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

/** The habit's instructions, growing with what is typed. ⌘↵ creates it. */
function Instructions({
  value,
  onChange,
  onSubmit,
}: {
  value: string;
  onChange: (value: string) => void;
  onSubmit: () => void;
}) {
  const t = useT();
  return (
    <AutoTextarea
      aria-label={t("monos.habit.whatItDoes")}
      value={value}
      rows={3}
      maxLength={4_000}
      placeholder={t("monos.newHabit.instructionsPlaceholder")}
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
