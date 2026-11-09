import { Button, Label, Picker } from "@expo/ui/swift-ui";
import { pickerStyle, tag } from "@expo/ui/swift-ui/modifiers";

import { SEARCH_BAR } from "@/features/files/FileSearchBar";
import { GlassMenuButton } from "@/ui/GlassMenuButton";

import type { InboxGrouping } from "./model";

/** The round button beside the search: how rows are grouped, and marking them read, in a native menu. */
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
    <GlassMenuButton
      icon="filter"
      symbol="slider.horizontal.3"
      label="Group by"
      size={SEARCH_BAR}
    >
      <Picker
        selection={value}
        onSelectionChange={(grouping) => onChange(grouping as InboxGrouping)}
        modifiers={[pickerStyle("inline")]}
      >
        <Label title="Day" systemImage="calendar" modifiers={[tag("day")]} />
        <Label title="Project" systemImage="folder" modifiers={[tag("project")]} />
      </Picker>
      {onMarkAllRead ? (
        <Button label="Mark all as read" systemImage="checkmark.circle" onPress={onMarkAllRead} />
      ) : null}
    </GlassMenuButton>
  );
}
