import { Label, Picker } from "@expo/ui/swift-ui";
import { pickerStyle, tag } from "@expo/ui/swift-ui/modifiers";

import { SEARCH_BAR } from "@/features/files/FileSearchBar";
import { GlassMenuButton } from "@/ui/GlassMenuButton";

import type { InboxGrouping } from "./model";

/** The round button beside the search: how rows are grouped, in a native menu. */
export function GroupMenu({
  value,
  onChange,
}: {
  value: InboxGrouping;
  onChange: (value: InboxGrouping) => void;
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
    </GlassMenuButton>
  );
}
