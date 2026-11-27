import { router, Stack } from "expo-router";
import { useEffect, useState } from "react";
import { ActivityIndicator, ScrollView, Text, View } from "react-native";
import { GestureHandlerRootView } from "react-native-gesture-handler";

import { useConnection } from "@/features/connection/ConnectionProvider";
import { VisualTree } from "@/features/visuals/VisualTree";
import type { VisualNode } from "@/features/visuals/types";
import { Markdown } from "@/features/sessions/Markdown";
import { themedStyles, useColors } from "@/ui/theme";

import { CodeHeaderTitle, HeaderBlur, HeaderButton } from "./codeScreen";
import { FileMissing } from "./FileMissing";

type MonoDocument = {
  title: string;
  kind: string;
  body: string;
  /** A visual's component tree, as the desktop renders it. */
  tree?: VisualNode;
  treeError?: string;
};

/**
 * A document a Mono wrote, read from the desktop as it opens and rendered as
 * the file sheet renders Markdown. A visual needs the desktop's renderer, so
 * it says so rather than show its source.
 */
export function DocumentScreen({
  documentId,
  title,
  stacked = false,
}: {
  documentId: string;
  title?: string;
  /** Pushed inside another sheet, as a Mono's profile: it goes back, not closes. */
  stacked?: boolean;
}) {
  const styles = useStyles();
  const colors = useColors();
  const { command } = useConnection();
  const [document, setDocument] = useState<MonoDocument | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    let disposed = false;
    command<MonoDocument>("mono.document", { documentId })
      .then((value) => {
        if (!disposed) setDocument(value);
      })
      .catch((reason: unknown) => {
        if (!disposed)
          setError(reason instanceof Error ? reason.message : String(reason));
      });
    return () => {
      disposed = true;
    };
  }, [command, documentId]);
  const visual = document?.kind === "html" || document?.kind === "visual";

  return (
    // A modal is its own native view hierarchy, so its gestures need their own root.
    <GestureHandlerRootView style={styles.root}>
      <Stack.Screen
        options={{
          headerTitle: () => (
            <CodeHeaderTitle name={document?.title || title || "Document"}>
              {visual ? "Visual" : "Document"}
            </CodeHeaderTitle>
          ),
          ...(stacked
            ? {}
            : {
                headerLeft: () => (
                  <HeaderButton icon="close" label="Close" onPress={() => router.back()} />
                ),
              }),
        }}
      />
      {document?.tree ? (
        // The desktop's dithered visual, drawn here from its rendered tree.
        <ScrollView
          style={styles.root}
          contentInsetAdjustmentBehavior="automatic"
          contentContainerStyle={styles.preview}
        >
          <VisualTree node={document.tree} />
        </ScrollView>
      ) : document && visual ? (
        <Text style={styles.note}>
          {document.treeError ?? "Open this visual on your desktop to see it."}
        </Text>
      ) : document ? (
        <ScrollView
          style={styles.root}
          contentInsetAdjustmentBehavior="automatic"
          contentContainerStyle={styles.preview}
        >
          <Markdown text={document.body} />
        </ScrollView>
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
    paddingTop: 120,
    color: colors.muted,
    fontSize: 15,
    textAlign: "center",
  },
}));
