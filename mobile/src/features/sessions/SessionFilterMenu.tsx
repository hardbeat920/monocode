import { Host, HStack, Label, Menu, Picker, Spacer } from "@expo/ui/swift-ui";
import {
  accessibilityLabel,
  contentShape,
  frame,
  pickerStyle,
  shapes,
  tag,
} from "@expo/ui/swift-ui/modifiers";
import { StyleSheet, View } from "react-native";

import { SEARCH_BAR } from "@/features/files/FileSearchBar";
import type { Harness, SessionSummary } from "@/features/workspace/types";
import { Icon } from "@/ui/Icon";
import { LiquidView } from "@/ui/LiquidView";
import { useColors } from "@/ui/theme";

export type SessionFilter = {
  /** Only the sessions working now. */
  working: boolean;
  /** Only one agent's sessions. */
  harness: Harness | null;
};

export const NO_FILTER: SessionFilter = { working: false, harness: null };

const HARNESS_NAMES: Record<Harness, string> = {
  claude: "Claude",
  codex: "Codex",
  cursor: "Cursor",
  grok: "Grok",
  opencode: "OpenCode",
  pi: "Pi",
  omp: "OMP",
  fx: "FX",
  hermes: "Hermes",
  antigravity: "Antigravity",
  devin: "Devin",
};

export function filterSessions(sessions: SessionSummary[], filter: SessionFilter) {
  if (!filter.working && !filter.harness) return sessions;
  return sessions.filter(
    (session) =>
      (!filter.working || session.busy) &&
      (!filter.harness || session.harness === filter.harness),
  );
}

/**
 * The round button beside the Sessions page's search: a native menu to show
 * only the sessions working now, or one agent's, from those in the list.
 * Lit while a filter is on.
 */
export function SessionFilterMenu({
  value,
  onChange,
  harnesses,
}: {
  value: SessionFilter;
  onChange: (value: SessionFilter) => void;
  /** The agents the list has sessions from. */
  harnesses: Harness[];
}) {
  const colors = useColors();
  const on = value.working || !!value.harness;
  return (
    <View>
      <LiquidView borderRadius={SEARCH_BAR / 2} innerStyle={styles.round}>
        <Icon name="filter" size={20} color={on ? colors.accent : colors.content} />
      </LiquidView>
      {/* The native menu lies over the glass, its label an invisible box of
          the button's size, as in WorkspaceMenu. */}
      <Host style={StyleSheet.absoluteFill}>
        <Menu
          modifiers={[accessibilityLabel(on ? "Filter sessions, on" : "Filter sessions")]}
          label={
            <HStack
              modifiers={[
                frame({ width: SEARCH_BAR, height: SEARCH_BAR }),
                contentShape(shapes.circle()),
              ]}
            >
              <Spacer />
            </HStack>
          }
        >
          <Picker
            selection={value.working ? "working" : "all"}
            onSelectionChange={(status) => onChange({ ...value, working: status === "working" })}
            modifiers={[pickerStyle("inline")]}
          >
            <Label title="All sessions" systemImage="tray.full" modifiers={[tag("all")]} />
            <Label title="Working now" systemImage="bolt" modifiers={[tag("working")]} />
          </Picker>
          {harnesses.length > 1 || value.harness ? (
            <Picker
              selection={value.harness ?? "any"}
              onSelectionChange={(harness) =>
                onChange({
                  ...value,
                  harness: harness === "any" ? null : (harness as Harness),
                })
              }
              modifiers={[pickerStyle("inline")]}
            >
              <Label title="Any agent" modifiers={[tag("any")]} />
              {harnesses.map((harness) => (
                <Label key={harness} title={HARNESS_NAMES[harness]} modifiers={[tag(harness)]} />
              ))}
            </Picker>
          ) : null}
        </Menu>
      </Host>
    </View>
  );
}

const styles = StyleSheet.create({
  round: {
    width: SEARCH_BAR,
    height: SEARCH_BAR,
  },
});
