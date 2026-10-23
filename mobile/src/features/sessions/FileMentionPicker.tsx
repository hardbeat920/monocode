import { useEffect, useRef } from "react";
import {
  Platform,
  Pressable,
  StyleSheet,
  ScrollView,
  Text,
  View,
  useWindowDimensions,
} from "react-native";
import { useKeyboardState } from "react-native-keyboard-controller";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import type { MentionFile } from "@/features/connection/useMentionSearch";
import { FileTypeIcon } from "@/features/files/FileTypeIcon";
import { LiquidView } from "@/ui/LiquidView";
import { themedStyles, useColors, useScheme } from "@/ui/theme";

export function FileMentionPicker({
  files,
  composerHeight,
  active,
  searching,
  error,
  onPick,
  onDismiss,
}: {
  files: MentionFile[];
  composerHeight: number;
  active: number;
  searching: boolean;
  error?: string;
  onPick: (file: MentionFile) => void;
  onDismiss: () => void;
}) {
  const styles = useStyles();
  const colors = useColors();
  const scheme = useScheme();
  const list = useRef<ScrollView>(null);
  const { height } = useWindowDimensions();
  const insets = useSafeAreaInsets();
  const keyboardHeight = useKeyboardState((state) => state.height);
  // In the keyboard-lifted dock's layout so Fabric can hit-test every row.
  // Reserve room for the input and screen header, including a tall attachment
  // draft. On resized Android windows this deliberately errs toward fewer rows.
  const available =
    height - insets.top - insets.bottom - keyboardHeight - composerHeight - 76;
  const maxHeight = Math.max(76, Math.min(220, available, height * 0.25));
  useEffect(() => {
    list.current?.scrollTo({
      y: Math.max(0, (active + 1) * 44 - (maxHeight - 32)),
      animated: false,
    });
  }, [active, maxHeight, files]);
  return (
    <View style={styles.panel} accessibilityLabel="File mentions">
      <View pointerEvents="none" style={StyleSheet.absoluteFill}>
        <LiquidView
          fill
          row={false}
          borderRadius={Platform.OS === "ios" ? 22 : 18}
          tintColor={
            Platform.OS === "android" && scheme === "light"
              ? "rgba(255,255,255,0.88)"
              : undefined
          }
          style={StyleSheet.absoluteFill}
        >
          <View />
        </LiquidView>
      </View>
      <View style={styles.header}>
        <Text style={styles.hint}>Files and folders</Text>
        <Pressable
          accessibilityRole="button"
          accessibilityLabel="Dismiss file suggestions"
          onPress={onDismiss}
          hitSlop={8}
        >
          <Text style={styles.hint}>Close</Text>
        </Pressable>
      </View>
      <ScrollView
        ref={list}
        style={{ maxHeight: Math.max(44, maxHeight - 32) }}
        keyboardShouldPersistTaps="always"
        keyboardDismissMode="none"
        nestedScrollEnabled
      >
        {files.length === 0 ? (
          <Text style={styles.empty}>
            {error
              ? `Could not search files: ${error}`
              : searching
                ? "Searching desktop files…"
                : "No matching files or folders"}
          </Text>
        ) : (
          files.map((file, index) => {
            const slash = file.path.lastIndexOf("/");
            const parent = slash < 0 ? "" : file.path.slice(0, slash);
            const offset = slash + 1;
            const positions = new Set(file.positions);
            return (
              <Pressable
                key={file.path}
                android_ripple={{ color: colors.selection }}
                accessibilityRole="button"
                accessibilityLabel={`${file.path}${file.isDir ? "/" : ""}`}
                accessibilityState={{ selected: active === index }}
                onPress={() => onPick(file)}
                style={[styles.row, active === index && styles.selected]}
              >
                <FileTypeIcon name={file.name} isDir={file.isDir} size={20} />
                <Text
                  style={[styles.name, active === index && styles.active]}
                  numberOfLines={1}
                >
                  {file.name.split("").map((char, i) => (
                    <Text
                      key={i}
                      style={
                        positions.has(offset + i) ? styles.match : undefined
                      }
                    >
                      {char}
                    </Text>
                  ))}
                  {file.isDir ? "/" : ""}
                </Text>
                {parent ? (
                  <Text
                    numberOfLines={1}
                    ellipsizeMode="middle"
                    style={styles.parent}
                  >
                    {parent}
                  </Text>
                ) : null}
              </Pressable>
            );
          })
        )}
      </ScrollView>
    </View>
  );
}
const useStyles = themedStyles((colors) => ({
  panel: {
    marginHorizontal: 12,
    marginBottom: 8,
    padding: 4,
    borderRadius: Platform.OS === "ios" ? 22 : 18,
    overflow: "hidden",
  },
  header: {
    height: 28,
    paddingHorizontal: 8,
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
  },
  hint: { color: colors.muted, fontSize: 12 },
  row: {
    height: 44,
    paddingHorizontal: 8,
    borderRadius: 9,
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
  },
  selected: { backgroundColor: colors.selection },
  name: { flexShrink: 1, color: colors.content, fontSize: 14 },
  active: { color: colors.accent },
  match: { color: colors.accent },
  parent: {
    flexShrink: 2,
    maxWidth: "45%",
    marginLeft: "auto",
    color: colors.muted,
    fontSize: 11,
  },
  empty: { color: colors.muted, fontSize: 13, padding: 10 },
}));
