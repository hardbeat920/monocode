import { router, Stack } from "expo-router";
import { useEffect, useMemo, useState } from "react";
import { ActivityIndicator, ScrollView, Text, View } from "react-native";
import { GestureHandlerRootView } from "react-native-gesture-handler";

import { splitPath } from "@/features/changes/diffTheme";
import { useConnection } from "@/features/connection/ConnectionProvider";
import { Markdown } from "@/features/sessions/Markdown";
import type { FileContents } from "@/features/workspace/types";
import { themedStyles, useColors } from "@/ui/theme";

import { CodeView, type CodeRow } from "./CodeView";
import { FileMissing } from "./FileMissing";
import {
  CodeHeaderTitle,
  HeaderButton,
  HeaderBlur,
  HeaderButtons,
  useWrapPreference,
} from "./codeScreen";

/** Files that open rendered rather than as source. */
const isMarkdown = (path: string) => /\.(md|mdx|markdown)$/i.test(path);

/** How often to collect a large file's colours while the desktop works on them. */
const HIGHLIGHT_POLL_MS = 250;

type Highlight = { palette: string[]; lines: [string, number][][] };

// Remembered between files for as long as the app runs.
let previewPreference = true;

/**
 * One file of the project, read from the desktop as it opens, in the
 * desktop's syntax colours when it could highlight it. A native modal whose
 * transparent header the code scrolls under,
 * line numbers that stay put while the code pans sideways, or wrapped lines.
 * Markdown opens rendered, with a switch to its source.
 */
export function FileScreen({
  projectId,
  path,
  worktreePath,
}: {
  projectId: string;
  path: string;
  worktreePath?: string;
}) {
  const styles = useStyles();
  const colors = useColors();
  const { command } = useConnection();
  const [file, setFile] = useState<FileContents | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let disposed = false;
    command<FileContents>("project.file", {
      projectId,
      path,
      ...(worktreePath ? { worktreePath } : {}),
    })
      .then((value) => {
        if (!disposed) setFile(value);
      })
      .catch((reason: unknown) => {
        if (!disposed)
          setError(reason instanceof Error ? reason.message : String(reason));
      });
    return () => {
      disposed = true;
    };
  }, [command, projectId, worktreePath, path]);

  // A large file's colours, collected from the top down as the desktop
  // finishes them, so it reads plain at once and colours in while open.
  const [progress, setProgress] = useState<Highlight | null>(null);
  const job = file?.highlightJob;
  useEffect(() => {
    if (!job) return;
    let disposed = false;
    let timer: ReturnType<typeof setTimeout>;
    let palette: string[] = [];
    let lines: [string, number][][] = [];
    const collect = async () => {
      try {
        const value = await command<Highlight & { done: boolean }>(
          "project.fileHighlight",
          {
            projectId,
            job,
            from: lines.length,
          },
        );
        if (disposed) return;
        if (value.lines.length > 0 || value.palette.length !== palette.length) {
          palette = value.palette;
          lines = lines.concat(value.lines);
          setProgress({ palette, lines });
        }
        if (value.done) return;
      } catch {
        // Missed this time; the next ask picks up from the same line.
        if (disposed) return;
      }
      timer = setTimeout(() => void collect(), HIGHLIGHT_POLL_MS);
    };
    void collect();
    return () => {
      disposed = true;
      clearTimeout(timer);
    };
  }, [command, projectId, job]);
  const highlight = file?.highlighted ?? (job ? progress : null);

  // A file named in a transcript is opened as written and found by the desktop.
  const found = file?.path ?? path;
  const { name, dir } = splitPath(found);
  const rows = useMemo<CodeRow[] | null>(() => {
    if (!file) return null;
    return file.text.split("\n").map((text, index) => ({
      number: index + 1,
      text,
      runs: highlight?.lines[index],
    }));
  }, [file, highlight]);
  const [wrap, toggleWrap] = useWrapPreference();
  const [preview, setPreview] = useState(previewPreference);
  const markdown = isMarkdown(found);
  const rendered = markdown && preview;

  return (
    // A modal is its own native view hierarchy, so its gestures need their own root.
    <GestureHandlerRootView style={styles.root}>
      <Stack.Screen
        options={{
          headerTitle: () => (
            <CodeHeaderTitle name={name}>
              {dir || "/"}
              {rows ? `  ${rows.length} lines` : ""}
            </CodeHeaderTitle>
          ),
          headerLeft: () => (
            <HeaderButton
              icon="close"
              label="Close"
              onPress={() => router.back()}
            />
          ),
          headerRight: () =>
            rows ? (
              <HeaderButtons>
                {markdown ? (
                  <HeaderButton
                    icon={preview ? "source" : "preview"}
                    label={preview ? "Show source" : "Show preview"}
                    onPress={() => setPreview((previewPreference = !preview))}
                  />
                ) : null}
                {rendered ? null : (
                  <HeaderButton
                    icon="wrap"
                    label={wrap ? "Don't wrap lines" : "Wrap lines"}
                    on={wrap}
                    onPress={toggleWrap}
                  />
                )}
              </HeaderButtons>
            ) : null,
        }}
      />
      {file && rendered ? (
        <ScrollView
          style={styles.root}
          contentInsetAdjustmentBehavior="automatic"
          contentContainerStyle={styles.preview}
        >
          <Markdown text={file.text} />
        </ScrollView>
      ) : rows ? (
        <CodeView
          rows={rows}
          palette={highlight?.palette}
          wrap={wrap}
          resetKey={path}
          footer={
            file?.truncated ? (
              <Text style={styles.note}>
                The rest of this file is too long to show here.
              </Text>
            ) : null
          }
        />
      ) : error ? (
        <FileMissing message={error} />
      ) : (
        <View style={styles.loading}>
          <ActivityIndicator color={colors.muted} />
        </View>
      )}
      <HeaderBlur color={colors.sheet} />
    </GestureHandlerRootView>
  );
}

const useStyles = themedStyles((colors) => ({
  root: {
    flex: 1,
  },
  loading: {
    flex: 1,
    alignItems: "center",
    justifyContent: "center",
  },
  preview: {
    paddingHorizontal: 20,
    paddingBottom: 40,
  },
  note: {
    paddingHorizontal: 20,
    paddingTop: 12,
    color: colors.muted,
    fontSize: 15,
  },
}));
