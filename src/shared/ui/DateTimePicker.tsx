import {
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
} from "react";
import { ChevronLeft, ChevronRight, Clock } from "./icons";

export function toLocalDateTime(date: Date): string {
  const pad = (part: number) => String(part).padStart(2, "0");
  return `${String(date.getFullYear()).padStart(4, "0")}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

export function parseLocalDateTime(value: string): Date | null {
  if (!/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}$/.test(value)) return null;
  const date = new Date(value);
  return Number.isFinite(date.getTime()) && toLocalDateTime(date) === value
    ? date
    : null;
}

type Props = {
  value: string;
  onChange: (value: string) => void;
  minDate?: string;
  autoFocus?: boolean;
};

function dateKey(date: Date): string {
  return toLocalDateTime(date).slice(0, 10);
}

function shiftMonth(date: Date, amount: number): Date {
  const lastDay = new Date(
    date.getFullYear(),
    date.getMonth() + amount + 1,
    0,
  ).getDate();
  return new Date(
    date.getFullYear(),
    date.getMonth() + amount,
    Math.min(date.getDate(), lastDay),
    12,
  );
}

export function DateTimePicker({
  value,
  onChange,
  minDate,
  autoFocus = false,
}: Props) {
  const today = new Date(Date.now());
  const selected = parseLocalDateTime(`${value.slice(0, 10)}T12:00`) ?? today;
  const minimum = minDate ? parseLocalDateTime(`${minDate}T12:00`) : null;
  const initial = minimum && selected < minimum ? minimum : selected;
  const [month, setMonth] = useState(
    () => new Date(initial.getFullYear(), initial.getMonth(), 1, 12),
  );
  const [focusedDate, setFocusedDate] = useState(initial);
  const gridRef = useRef<HTMLDivElement>(null);
  const shouldFocus = useRef(autoFocus);
  const minimumTime = minimum?.getTime();
  useLayoutEffect(() => {
    if (minimumTime === undefined || focusedDate.getTime() >= minimumTime)
      return;
    shouldFocus.current = !!gridRef.current?.contains(document.activeElement);
    const next = new Date(minimumTime);
    setFocusedDate(next);
    setMonth(new Date(next.getFullYear(), next.getMonth(), 1, 12));
  }, [minimumTime, focusedDate]);
  useLayoutEffect(() => {
    if (!shouldFocus.current) return;
    shouldFocus.current = false;
    gridRef.current
      ?.querySelector<HTMLButtonElement>('button[tabindex="0"]')
      ?.focus();
  }, [focusedDate, month]);
  const previousDisabled =
    !!minimum &&
    month.getFullYear() * 12 + month.getMonth() <=
      minimum.getFullYear() * 12 + minimum.getMonth();
  const monthLabel = month.toLocaleDateString(undefined, { month: "long" });
  const headingId = useId();
  const timeId = useId();
  const timeHintId = useId();
  const selectedKey = value.slice(0, 10);
  const time = value.includes("T") ? value.slice(value.indexOf("T") + 1) : "";
  const navigate = (date: Date, focus: boolean) => {
    const next = minimum && date < minimum ? minimum : date;
    shouldFocus.current = focus;
    setFocusedDate(next);
    setMonth(new Date(next.getFullYear(), next.getMonth(), 1, 12));
  };
  const pick = (date: Date) => {
    setFocusedDate(date);
    onChange(`${dateKey(date)}T${time}`);
  };
  const onDayKeyDown = (
    event: KeyboardEvent<HTMLButtonElement>,
    date: Date,
  ) => {
    const dayOffset = (date.getDay() + 6) % 7;
    const offsets: Record<string, number> = {
      ArrowLeft: -1,
      ArrowRight: 1,
      ArrowUp: -7,
      ArrowDown: 7,
      Home: -dayOffset,
      End: 6 - dayOffset,
    };
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      event.stopPropagation();
      pick(date);
      return;
    }
    let next: Date;
    if (event.key === "PageUp" || event.key === "PageDown") {
      next = shiftMonth(date, event.key === "PageUp" ? -1 : 1);
    } else if (event.key in offsets) {
      next = new Date(
        date.getFullYear(),
        date.getMonth(),
        date.getDate() + offsets[event.key],
        12,
      );
    } else return;
    event.preventDefault();
    event.stopPropagation();
    navigate(next, true);
  };
  const offset = (month.getDay() + 6) % 7;
  const count = new Date(
    month.getFullYear(),
    month.getMonth() + 1,
    0,
  ).getDate();
  const cells = Array.from(
    { length: Math.ceil((offset + count) / 7) * 7 },
    (_, i) => {
      const number = i - offset + 1;
      return number > 0 && number <= count
        ? new Date(month.getFullYear(), month.getMonth(), number, 12)
        : null;
    },
  );
  return (
    <div>
      <div className="mb-2 flex h-8 items-center justify-between px-1">
        <span
          id={headingId}
          aria-live="polite"
          className="flex items-baseline gap-1.5 text-xs font-medium text-content/90"
        >
          {monthLabel}{" "}
          <span className="font-normal tabular-nums text-content/40">
            {month.getFullYear()}
          </span>
        </span>
        <div className="flex gap-0.5">
          <button
            type="button"
            aria-label="Previous month"
            disabled={previousDisabled}
            onClick={() => navigate(shiftMonth(focusedDate, -1), false)}
            className="grid size-7 place-items-center rounded text-content/55 hover:bg-content/5 hover:text-content focus-visible:outline-2 focus-visible:outline-accent disabled:pointer-events-none disabled:opacity-30"
          >
            <ChevronLeft className="size-3.5" />
          </button>
          <button
            type="button"
            aria-label="Next month"
            onClick={() => navigate(shiftMonth(focusedDate, 1), false)}
            className="grid size-7 place-items-center rounded text-content/55 hover:bg-content/5 hover:text-content focus-visible:outline-2 focus-visible:outline-accent"
          >
            <ChevronRight className="size-3.5" />
          </button>
        </div>
      </div>
      <div ref={gridRef} role="grid" aria-labelledby={headingId}>
        <div role="row" className="grid grid-cols-7">
          {["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"].map((label) => (
            <span
              key={label}
              role="columnheader"
              className="pb-1.5 text-center text-[10px] font-normal text-content/40"
            >
              {label}
            </span>
          ))}
        </div>
        {Array.from({ length: cells.length / 7 }, (_, week) => (
          <div role="row" key={week} className="grid grid-cols-7 py-0.5">
            {cells.slice(week * 7, week * 7 + 7).map((date, column) => (
              <div
                role="gridcell"
                key={column}
                className="flex justify-center"
                aria-selected={date ? dateKey(date) === selectedKey : undefined}
              >
                {date ? (
                  <button
                    type="button"
                    aria-label={dateKey(date)}
                    tabIndex={dateKey(date) === dateKey(focusedDate) ? 0 : -1}
                    onKeyDown={(event) => onDayKeyDown(event, date)}
                    title={date.toLocaleDateString(undefined, {
                      dateStyle: "full",
                    })}
                    disabled={!!minimum && date < minimum}
                    aria-current={
                      dateKey(date) === dateKey(today) ? "date" : undefined
                    }
                className={`relative size-8 rounded text-xs tabular-nums focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent disabled:pointer-events-none disabled:text-content/20 ${dateKey(date) === selectedKey ? "bg-selection-hover font-medium text-content ring-1 ring-inset ring-content/20" : "text-content/70 hover:bg-content/5 hover:text-content"}`}
                    onClick={() => pick(date)}
                  >
                    {date.getDate()}
                    {dateKey(date) === dateKey(today) ? (
                      <span
                        aria-hidden="true"
                        className="absolute bottom-1 left-1/2 size-0.5 -translate-x-1/2 rounded-full bg-current"
                      />
                    ) : null}
                  </button>
                ) : null}
              </div>
            ))}
          </div>
        ))}
      </div>
      <div className="mt-3 flex items-center justify-between gap-3 border-t border-stroke px-1 pt-3">
        <div>
          <label
            htmlFor={timeId}
            className="flex items-center gap-1.5 text-xs text-content/70"
          >
            <Clock
              className="size-3 shrink-0 text-content/40"
              aria-hidden="true"
            />
            Time
          </label>
          <p id={timeHintId} className="mt-0.5 text-[10px] text-content/40">
            Local time, 24-hour
          </p>
        </div>
        <div className="w-20 rounded border border-content/10 bg-content/5 focus-within:border-content/40 focus-within:outline-2 focus-within:outline-accent">
          <input
            id={timeId}
            type="text"
            aria-describedby={timeHintId}
            autoComplete="off"
            spellCheck={false}
            placeholder="HH:mm"
            value={time}
            onChange={(event) =>
              onChange(`${dateKey(selected)}T${event.target.value}`)
            }
            className="w-full bg-transparent px-2 py-1.5 text-center font-mono text-xs tabular-nums text-content outline-none placeholder:text-content/30"
          />
        </div>
      </div>
    </div>
  );
}
