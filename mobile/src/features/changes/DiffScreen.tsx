import { router, Stack } from "expo-router";
import { useMemo } from "react";
import { ScrollView, Text } from "react-native";
import { GestureHandlerRootView } from "react-native-gesture-handler";

import { CodeView, type CodeRow } from "@/features/files/CodeView";
import {
  CodeHeaderTitle,
  HeaderBlur,
  HeaderButton,
  useWrapPreference,
} from "@/features/files/codeScreen";
import type { ChangedFile } from "@/features/workspace/types";
import { themedStyles, useColors } from "@/ui/theme";

import { formatCount, splitPath, STATUS_LETTER, useDiffTheme } from "./diffTheme";

/**
 * One file's unified diff, styled after the desktop UnifiedDiffView, in the
 * desktop's syntax colours when it could highlight the file: a native modal
 * whose transparent header the diff scrolls under. As in the file screen, the
 * line numbers and signs stay put while the code pans sideways, or long lines
 * wrap. It shows the diff as it was when opened.
 */
export function DiffScreen({ file }: { file: ChangedFile | undefined }) {
  const colors = useColors();
  const styles = useStyles();
  const { name, dir } = splitPath(file?.relative ?? "");
  const diff = useDiffTheme();
  const [wrap, toggleWrap] = useWrapPreference();
  const rows = useMemo<CodeRow[]>(
    () =>
      (file?.lines ?? []).map((line) =>
        line.kind === "hunk"
          ? { number: null, text: line.text, hunk: true }
          : {
              number: line.kind === "del" ? line.oldNumber : line.newNumber,
              text: line.text,
              runs: line.runs,
              sign: line.kind === "add" ? "+" : line.kind === "del" ? "−" : " ",
              tone: line.kind === "context" ? null : diff[line.kind],
              dim: line.kind === "context",
            },
      ),
    [file, diff],
  );

  return (
    // A modal is its own native view hierarchy, so its gestures need their own root.
    <GestureHandlerRootView style={styles.root}>
      <Stack.Screen
        options={{
          headerTitle: () => (
            <CodeHeaderTitle name={name}>
              {file ? (
                <Text style={{ color: diff.status[file.status] }}>
                  {STATUS_LETTER[file.status]}{" "}
                </Text>
              ) : null}
              {dir || "/"}
              {"  "}
              {file?.additions ? (
                <Text style={{ color: diff.add.fg }}>+{formatCount(file.additions)} </Text>
              ) : null}
              {file?.deletions ? (
                <Text style={{ color: diff.del.fg }}>-{formatCount(file.deletions)}</Text>
              ) : null}
            </CodeHeaderTitle>
          ),
          headerLeft: () => (
            <HeaderButton icon="close" label="Close" onPress={() => router.back()} />
          ),
          headerRight: () => (
            <HeaderButton
              icon="wrap"
              label={wrap ? "Don't wrap lines" : "Wrap lines"}
              on={wrap}
              onPress={toggleWrap}
            />
          ),
        }}
      />
      {file ? (
        <CodeView rows={rows} palette={file.palette} wrap={wrap} resetKey={file.relative} />
      ) : (
        // Opened again after the app restarted, say, with the diff gone.
        <ScrollView style={styles.root} contentInsetAdjustmentBehavior="automatic">
          <Text style={styles.note}>This diff is no longer available. Open it again from Changes.</Text>
        </ScrollView>
      )}
      <HeaderBlur color={colors.sheet} />
    </GestureHandlerRootView>
  );
}

const useStyles = themedStyles((colors) => ({
  root: {
    flex: 1,
  },
  note: {
    paddingHorizontal: 20,
    paddingTop: 12,
    color: colors.muted,
    fontSize: 15,
  },
}));
