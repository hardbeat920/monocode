import type { TaskListItem, TaskListItemStatus } from "./session";

export function isTaskListToolName(value: string): boolean {
  const name = value.trim().toLowerCase().replace(/[\s_-]+/g, "");
  return ["todowrite", "writetodos", "updatetodos"].some(
    (candidate) => name === candidate || name.endsWith(candidate),
  );
}

/** Common todo-write payload used by Claude, OpenCode, and Pi extensions. */
export function taskListFromToolInput(
  toolName: string,
  input: unknown,
): TaskListItem[] | null {
  if (!isTaskListToolName(toolName)) return null;
  const rec = asRecord(input);
  const todos = rec?.todos;
  if (!Array.isArray(todos)) return null;
  return todos.flatMap((todo): TaskListItem[] => {
    const row = asRecord(todo);
    const text = [row?.content, row?.activeForm, row?.text]
      .find(
        (value): value is string =>
          typeof value === "string" && !!value.trim(),
      )
      ?.trim();
    if (!text) return [];
    const id = taskListItemId(row?.id);
    return [
      {
        ...(id ? { id } : {}),
        text,
        status: normalizeTaskListStatus(row?.status),
      },
    ];
  });
}

export function normalizeTaskListStatus(value: unknown): TaskListItemStatus {
  const status = String(value ?? "pending")
    .trim()
    .toLowerCase()
    .replace(/[\s_-]+/g, "");
  if (status === "completed" || status === "complete" || status === "done") {
    return "completed";
  }
  if (status === "inprogress" || status === "active" || status === "running") {
    return "in_progress";
  }
  if (status === "cancelled" || status === "canceled" || status === "skipped") {
    return "cancelled";
  }
  return "pending";
}

export function taskListText(items: TaskListItem[]): string {
  return items
    .map((item) => `${taskListMark(item.status)} ${item.text}`)
    .join("\n");
}

/** Read task snapshots persisted before task lists had their own block role. */
export function legacyTaskListFromText(text: string): TaskListItem[] | null {
  const lines = text
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);
  if (lines.length === 0) return null;
  const items = lines.map((line) => {
    const match = line.match(/^\[([xX ~…-])\]\s+(.+)$/);
    if (!match) return null;
    return {
      text: match[2].trim(),
      status: statusFromLegacyMark(match[1]),
    } satisfies TaskListItem;
  });
  return items.every((item): item is TaskListItem => item !== null)
    ? items
    : null;
}

export function taskListProgressLabel(items: TaskListItem[]): string {
  const completed = items.filter((item) => item.status === "completed").length;
  const actionable = items.filter((item) => item.status !== "cancelled").length;
  if (actionable > 0 && completed === actionable) return "Complete";
  return `${completed} of ${actionable || items.length}`;
}

function taskListMark(status: TaskListItemStatus): string {
  if (status === "completed") return "[x]";
  if (status === "in_progress") return "[~]";
  if (status === "cancelled") return "[-]";
  return "[ ]";
}

function statusFromLegacyMark(mark: string): TaskListItemStatus {
  if (mark.toLowerCase() === "x") return "completed";
  if (mark === "~" || mark === "…") return "in_progress";
  if (mark === "-") return "cancelled";
  return "pending";
}

function asRecord(value: unknown): Record<string, unknown> | null {
  if (value && typeof value === "object" && !Array.isArray(value)) {
    return value as Record<string, unknown>;
  }
  return null;
}

function taskListItemId(value: unknown): string | undefined {
  if (typeof value === "string") return value.trim() || undefined;
  if (typeof value === "number" && Number.isFinite(value)) return String(value);
  return undefined;
}
