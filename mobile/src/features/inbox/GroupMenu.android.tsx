import { SEARCH_BAR } from "@/features/files/FileSearchBar";
import { ChoiceMenu } from "@/ui/ChoiceMenu";
import { Icon } from "@/ui/Icon";
import { LiquidView } from "@/ui/LiquidView";

import type { InboxGrouping } from "./model";

/** The round button beside the search: how rows are grouped. */
export function GroupMenu({
  value,
  onChange,
}: {
  value: InboxGrouping;
  onChange: (value: InboxGrouping) => void;
}) {
  return (
    <ChoiceMenu
      label="Group by"
      actions={[
        { label: "Day", selected: value === "day", onPress: () => onChange("day") },
        { label: "Project", selected: value === "project", onPress: () => onChange("project") },
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
