import type { ExplorerMenuItem } from "./ExplorerMenu";
import { reminderTime } from "../lib/sessionReminders";

export function sessionReminderPresets(now = new Date()) {
  const timeInHours = (hours: 1 | 3) => {
    const date = new Date(reminderTime(`reminder:${hours}h`, now)!);
    return `${date.getHours()}:${String(date.getMinutes()).padStart(2, "0")}`;
  };

  return [
    { kind: "item", id: "reminder:1h", label: `In 1 hour (${timeInHours(1)})` },
    {
      kind: "item",
      id: "reminder:3h",
      label: `In 3 hours (${timeInHours(3)})`,
    },
    {
      kind: "item",
      id: "reminder:evening",
      label: "This evening (18:00)",
      disabled: reminderTime("reminder:evening", now) == null,
    },
    { kind: "item", id: "reminder:tomorrow", label: "Tomorrow (9:00)" },
    { kind: "item", id: "reminder:next-week", label: "Next week (Mon 9:00)" },
  ] satisfies ExplorerMenuItem[];
}
