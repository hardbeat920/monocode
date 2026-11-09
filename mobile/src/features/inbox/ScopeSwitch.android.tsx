import { Text, View } from "react-native";

import { GlassSegments } from "@/ui/GlassSegments";
import { CONTROL_TEXT_MAX } from "@/ui/textScale";
import { themedStyles } from "@/ui/theme";

import { INBOX_SCOPES } from "./model";
import type { InboxScope } from "./protocol";

/** All / Needs me / Done, in the same glass as the page switch above. */
export function ScopeSwitch({
  value,
  onChange,
  needsMe,
}: {
  value: InboxScope;
  onChange: (value: InboxScope) => void;
  needsMe: number;
}) {
  const styles = useStyles();
  return (
    <GlassSegments
      value={value}
      onChange={onChange}
      segments={INBOX_SCOPES.map((scope) => ({
        value: scope.id,
        label: scope.label,
        trailing:
          scope.id === "needsMe" && needsMe > 0 ? (
            <View style={styles.count}>
              <Text maxFontSizeMultiplier={CONTROL_TEXT_MAX} style={styles.countLabel}>
                {needsMe}
              </Text>
            </View>
          ) : undefined,
      }))}
    />
  );
}

const useStyles = themedStyles((colors) => ({
  count: {
    minWidth: 18,
    minHeight: 18,
    paddingHorizontal: 5,
    borderRadius: 11,
    alignItems: "center",
    justifyContent: "center",
    backgroundColor: colors.accent,
  },
  countLabel: {
    color: colors.inverse,
    fontSize: 11,
    fontWeight: "700",
  },
}));
