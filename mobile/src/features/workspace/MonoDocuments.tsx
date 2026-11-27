import { router } from "expo-router";
import { Fragment } from "react";
import { Pressable, StyleSheet, Text, View } from "react-native";

import { Icon } from "@/ui/Icon";
import { themedStyles, useColors } from "@/ui/theme";

import type { MonoProfile } from "./monoProfile";

type MonoDocument = MonoProfile["documents"][number];

/** How long ago, as a chat list says it: now, then minutes, hours, days. */
export function ago(ms: number) {
  const seconds = Math.max(0, (Date.now() - ms) / 1000);
  if (seconds < 60) return "now";
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h`;
  return `${Math.floor(seconds / 86400)}d`;
}

/**
 * The documents a Mono wrote, newest first, as rows of a settings card: each
 * opens beside the profile, inside its sheet. With `limit`, only that many,
 * and a row under them to see all of them on a page of their own.
 */
export function MonoDocumentRows({
  monoId,
  documents,
  limit,
}: {
  monoId: string;
  documents: MonoDocument[];
  limit?: number;
}) {
  const styles = useStyles();
  const colors = useColors();
  if (!documents.length) return <Text style={styles.empty}>No documents yet.</Text>;
  const newest = [...documents].sort((a, b) => b.updatedAt - a.updatedAt);
  const shown = limit === undefined ? newest : newest.slice(0, limit);
  const more = newest.length - shown.length;
  return (
    <>
      {shown.map((document, index) => (
        <Fragment key={document.id}>
          {index > 0 ? <View style={styles.divider} /> : null}
          <Pressable
            accessibilityRole="button"
            accessibilityLabel={`Open ${document.title}`}
            onPress={() =>
              router.push({
                pathname: "/mono-profile/[monoId]/document",
                params: { monoId, documentId: document.id, title: document.title },
              })
            }
            style={({ pressed }) => [styles.row, pressed && styles.pressed]}
          >
            <View style={styles.icon}>
              <Icon
                name={["html", "visual"].includes(document.kind) ? "metrics" : "edit"}
                size={18}
                color={colors.content}
              />
            </View>
            <Text style={styles.title} numberOfLines={1}>
              {document.title}
            </Text>
            <Text style={styles.detail}>{ago(document.updatedAt)}</Text>
          </Pressable>
        </Fragment>
      ))}
      {more > 0 ? (
        <>
          <View style={styles.divider} />
          <Pressable
            accessibilityRole="button"
            accessibilityLabel={`See all ${newest.length} documents`}
            onPress={() =>
              router.push({ pathname: "/mono-profile/[monoId]/documents", params: { monoId } })
            }
            style={({ pressed }) => [styles.row, pressed && styles.pressed]}
          >
            <View style={styles.icon}>
              <Icon name="list" size={18} color={colors.accent} />
            </View>
            <Text style={[styles.title, styles.more]}>See all</Text>
            <Text style={styles.detail}>{newest.length}</Text>
            <Icon name="chevronRight" size={16} color={colors.muted} />
          </Pressable>
        </>
      ) : null}
    </>
  );
}

const useStyles = themedStyles((colors) => ({
  // As a settings row, so documents sit like the projects do.
  row: {
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
    minHeight: 52,
    paddingVertical: 8,
    paddingRight: 8,
  },
  icon: {
    width: 36,
    height: 36,
    alignItems: "center",
    justifyContent: "center",
  },
  title: {
    flex: 1,
    color: colors.content,
    fontSize: 17,
  },
  more: {
    color: colors.accent,
  },
  detail: {
    color: colors.muted,
    fontSize: 13,
  },
  pressed: {
    opacity: 0.6,
  },
  divider: {
    height: StyleSheet.hairlineWidth,
    marginLeft: 40,
    backgroundColor: colors.stroke,
  },
  empty: {
    color: colors.muted,
    fontSize: 14,
    lineHeight: 20,
    paddingHorizontal: 8,
    paddingVertical: 12,
  },
}));
