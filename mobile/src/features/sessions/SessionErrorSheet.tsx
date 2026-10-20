import { BottomSheet, RNHostView } from "@expo/ui";
import { ScrollView, StyleSheet, Text, useWindowDimensions, View } from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";

import { Icon } from "@/ui/Icon";
import { LiquidView } from "@/ui/LiquidView";
import { themedStyles, useColors, withAlpha } from "@/ui/theme";

import { sessionErrorContent } from "./sessionError";

/** One sheet per transcript, outside the virtualized rows so it stays mounted. */
export function SessionErrorSheet({
  message,
  isPresented,
  onDismiss,
}: {
  message: string;
  isPresented: boolean;
  onDismiss: () => void;
}) {
  const colors = useColors();
  const styles = useStyles();
  const { width, height } = useWindowDimensions();
  const { bottom } = useSafeAreaInsets();
  const content = sessionErrorContent(message);
  return (
    <BottomSheet
      isPresented={isPresented}
      onDismiss={onDismiss}
      contentPadding={0}
      containerColor={colors.sheet}
    >
      <RNHostView matchContents={{ vertical: true }}>
        <View style={[styles.sheet, { width, paddingBottom: Math.max(bottom, 16) + 8 }]}>
          <View style={styles.badge}>
            <Icon name={content.recovery ? "lock" : "chat"} size={30} color={colors.accent} />
          </View>
          <Text accessibilityRole="header" style={styles.title}>{content.title}</Text>
          <ScrollView
            style={[styles.details, { maxHeight: height * 0.35 }]}
            contentContainerStyle={styles.detailsContent}
            showsVerticalScrollIndicator={false}
          >
            <Text style={styles.message} selectable>{content.message}</Text>
            {content.recovery ? (
              <View style={styles.recovery}>
                <Icon name="computer" size={22} color={colors.muted} />
                <View style={styles.recoveryText}>
                  <Text style={styles.recoveryTitle}>Switch accounts</Text>
                  <Text style={styles.recoveryMessage}>{content.recovery}</Text>
                </View>
              </View>
            ) : null}
          </ScrollView>
          <LiquidView
            onPress={onDismiss}
            accessibilityLabel="Got it"
            pressedScale={1.03}
            style={styles.button}
            innerStyle={styles.buttonSurface}
          >
            <Text style={styles.buttonLabel}>Got it</Text>
          </LiquidView>
        </View>
      </RNHostView>
    </BottomSheet>
  );
}

const useStyles = themedStyles((colors) => ({
  sheet: {
    alignItems: "center",
    paddingTop: 32,
    paddingHorizontal: 24,
  },
  badge: {
    width: 72,
    height: 72,
    borderRadius: 36,
    alignItems: "center",
    justifyContent: "center",
    backgroundColor: withAlpha(colors.accent, 0.09),
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: withAlpha(colors.accent, 0.16),
    marginBottom: 20,
  },
  title: {
    color: colors.content,
    fontSize: 24,
    fontWeight: "600",
    textAlign: "center",
  },
  details: {
    alignSelf: "stretch",
    marginTop: 10,
    marginBottom: 24,
  },
  detailsContent: {
    gap: 24,
  },
  message: {
    color: colors.muted,
    fontSize: 16,
    lineHeight: 23,
    textAlign: "center",
  },
  recovery: {
    flexDirection: "row",
    alignItems: "flex-start",
    gap: 12,
    padding: 16,
    borderRadius: 18,
    borderCurve: "continuous",
    backgroundColor: withAlpha(colors.content, 0.04),
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: colors.stroke,
  },
  recoveryText: {
    flex: 1,
    gap: 5,
  },
  recoveryTitle: {
    color: colors.content,
    fontSize: 14,
    fontWeight: "600",
    lineHeight: 20,
  },
  recoveryMessage: {
    color: colors.muted,
    fontSize: 14,
    lineHeight: 20,
  },
  button: {
    alignSelf: "stretch",
  },
  buttonSurface: {
    minHeight: 52,
    paddingVertical: 14,
    paddingHorizontal: 24,
  },
  buttonLabel: {
    color: colors.content,
    fontSize: 16,
    fontWeight: "600",
  },
}));
