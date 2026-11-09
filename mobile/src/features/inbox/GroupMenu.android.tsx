import { SEARCH_BAR } from "@/features/files/FileSearchBar";
import { ChoiceMenu } from "@/ui/ChoiceMenu";
import { Icon } from "@/ui/Icon";
import { LiquidView } from "@/ui/LiquidView";

import type { InboxGrouping } from "./model";

/** The round button beside the search: how rows are grouped, and marking them read. */
export function GroupMenu({
  value,
  onChange,
  onMarkAllRead,
}: {
  value: InboxGrouping;
  onChange: (value: InboxGrouping) => void;
  /** Left out when nothing is unread. */
  onMarkAllRead?: () => void;
}) {
  return (
    <ChoiceMenu
      label="Group by"
      sections={[
        [
          { label: "Day", selected: value === "day", onPress: () => onChange("day") },
          { label: "Project", selected: value === "project", onPress: () => onChange("project") },
        ],
        onMarkAllRead ? [{ label: "Mark all as read", icon: "check", onPress: onMarkAllRead }] : [],
      ]}
    >
      <LiquidView
        borderRadius={SEARCH_BAR / 2}
        innerStyle={{ width: SEARCH_BAR, height: SEARCH_BAR }}
      >
        <Icon name="filter" size={20} />
      </LiquidView>
    </ChoiceMenu>
  );
}
