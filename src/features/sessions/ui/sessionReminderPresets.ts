import type { ExplorerMenuItem } from "../../files/ui/ExplorerMenu";
import { reminderTime } from "../model/sessionReminders";
import { t } from "../../../shared/i18n/locale";

export function sessionReminderPresets(now = new Date()) {
  const timeInHours = (hours: 1 | 3) => {
    const date = new Date(reminderTime(`reminder:${hours}h`, now)!);
    return `${date.getHours()}:${String(date.getMinutes()).padStart(2, "0")}`;
  };

  return [
    {
      kind: "item",
      id: "reminder:1h",
      label: t("sessions.reminders.inOneHour", { time: timeInHours(1) }),
    },
    {
      kind: "item",
      id: "reminder:3h",
      label: t("sessions.reminders.inThreeHours", { time: timeInHours(3) }),
    },
    {
      kind: "item",
      id: "reminder:evening",
      label: t("sessions.reminders.thisEvening"),
      disabled: reminderTime("reminder:evening", now) == null,
    },
    {
      kind: "item",
      id: "reminder:tomorrow",
      label: t("sessions.reminders.tomorrow"),
    },
    {
      kind: "item",
      id: "reminder:next-week",
      label: t("sessions.reminders.nextWeek"),
    },
  ] satisfies ExplorerMenuItem[];
}
