import { Check, ListEnd, Loader, Minus } from "./icons";
import type { TaskListItem, TaskListItemStatus } from "../lib/session";
import { taskListProgressLabel } from "../lib/taskList";

type Props = {
  items: TaskListItem[];
  explanation?: string;
};

export function TaskListPreview({ items, explanation }: Props) {
  return (
    <section
      aria-label="Task progress"
      className="mb-2 overflow-hidden rounded-[10px] border border-content/10 bg-content/[0.035]"
    >
      <div className="flex items-start gap-2 border-b border-content/8 px-2.5 py-2">
        <ListEnd
          className="mt-0.5 size-4 shrink-0 text-content/45"
          strokeWidth={1.75}
        />
        <div className="min-w-0 flex-1">
          <div className="flex items-center justify-between gap-3">
            <h3 className="font-mono text-[12px] font-medium text-content/85">
              Tasks
            </h3>
            <span className="shrink-0 rounded-full bg-content/7 px-2 py-0.5 font-mono text-[10px] text-content/50">
              {taskListProgressLabel(items)}
            </span>
          </div>
          {explanation ? (
            <p className="mt-0.5 line-clamp-2 font-sans text-[11.5px] leading-4 text-content/50">
              {explanation}
            </p>
          ) : null}
        </div>
      </div>
      <ol className="py-1">
        {items.map((item, index) => (
          <li
            key={item.id ?? `${index}:${item.text}`}
            className="flex min-w-0 items-start gap-2.5 px-2.5 py-1.5"
          >
            <TaskState status={item.status} />
            <span
              className={`min-w-0 flex-1 font-sans text-[12.5px] leading-4.5 ${
                item.status === "completed"
                  ? "text-content/40 line-through decoration-content/25"
                  : item.status === "cancelled"
                    ? "text-content/35 line-through decoration-content/20"
                    : item.status === "in_progress"
                      ? "text-content/85"
                      : "text-content/60"
              }`}
            >
              {item.text}
            </span>
          </li>
        ))}
      </ol>
    </section>
  );
}

function TaskState({ status }: { status: TaskListItemStatus }) {
  if (status === "completed") {
    return (
      <span
        aria-label="Completed"
        className="mt-px grid size-4 shrink-0 place-items-center rounded-full bg-emerald-400/20 text-emerald-300"
      >
        <Check className="size-2.5" strokeWidth={2.5} />
      </span>
    );
  }
  if (status === "in_progress") {
    return (
      <span
        aria-label="In progress"
        className="mt-px grid size-4 shrink-0 place-items-center text-sky-300"
      >
        <Loader
          className="size-4 motion-safe:animate-spin"
          strokeWidth={2}
        />
      </span>
    );
  }
  if (status === "cancelled") {
    return (
      <span
        aria-label="Cancelled"
        className="mt-px grid size-4 shrink-0 place-items-center rounded-full bg-content/8 text-content/35"
      >
        <Minus className="size-2.5" strokeWidth={2} />
      </span>
    );
  }
  return (
    <span
      aria-label="Pending"
      className="mt-px size-4 shrink-0 rounded-full border border-content/25 bg-content/[0.02]"
    />
  );
}
