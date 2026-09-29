import { t } from "../../i18n/model/i18n";
import type { ExplorerMenuItem } from "../../files/ui/ExplorerMenu";
import { reminderTime } from "../model/sessionReminders";

export function sessionReminderPresets(now = new Date()) {
  const timeInHours = (hours: 1 | 3) => {
    const date = new Date(reminderTime(`reminder:${hours}h`, now)!);
    return `${date.getHours()}:${String(date.getMinutes()).padStart(2, "0")}`;
  };

  return [
    { kind: "item", id: "reminder:1h", label: t("sessions.reminders.in1Hour", "In 1 hour ({time})", { time: timeInHours(1) }) },
    {
      kind: "item",
      id: "reminder:3h",
      label: t("sessions.reminders.in3Hours", "In 3 hours ({time})", { time: timeInHours(3) }),
    },
    {
      kind: "item",
      id: "reminder:evening",
      label: t("session.reminderPresets.thisEvening", "This evening (18:00)"),
      disabled: reminderTime("reminder:evening", now) == null,
    },
    { kind: "item", id: "reminder:tomorrow", label: t("session.reminderPresets.tomorrow", "Tomorrow (9:00)") },
    { kind: "item", id: "reminder:next-week", label: t("session.reminderPresets.nextWeek", "Next week (Mon 9:00)") },
  ] satisfies ExplorerMenuItem[];
}
