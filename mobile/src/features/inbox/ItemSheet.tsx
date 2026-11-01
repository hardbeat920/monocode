import * as Haptics from "expo-haptics";
import { router, Stack } from "expo-router";
import { useEffect, useMemo, useState } from "react";
import { ActivityIndicator, Linking, Pressable, Text, View } from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";

import { keepDiff } from "@/features/changes/shownDiffs";
import { HeaderButton } from "@/features/files/codeScreen";
import { Markdown } from "@/features/sessions/Markdown";
import { SettingsPage } from "@/features/settings/SettingsUI";
import { relativeTime } from "@/features/workspace/relativeTime";
import type { ChangedFile } from "@/features/workspace/types";
import { Icon } from "@/ui/Icon";
import { themedStyles, useColors, withAlpha } from "@/ui/theme";

import { Avatar } from "./Avatar";
import { hasChecks, hasCode, useInboxDetail } from "./detail";
import { useInbox } from "./InboxProvider";
import { ChecksGlyph, whyLabel } from "./InboxRow";
import { InboxToast } from "./InboxToast";
import { ProviderMark, SOURCE_NAMES } from "./ProviderMark";
import type {
  InboxCheck,
  InboxDetail,
  InboxDiffFile,
  InboxItem,
  InboxThreadEntry,
} from "./protocol";
import { useNow } from "./useNow";

type Tab = "summary" | "code" | "checks";

/**
 * An inbox item's sheet, as the mock lays it out: provider, number, status
 * and author; the description and comments; for a pull request its changed
 * files and, on GitHub, its checks. The primary action starts an agent on
 * it; the other opens it in the provider's own app or site.
 */
export function ItemSheet({ itemKey }: { itemKey: string }) {
  const colors = useColors();
  const styles = useStyles();
  const { bottom } = useSafeAreaInsets();
  const { client, mark } = useInbox();
  const { item, detail, diff, checks, error, loading, reload } = useInboxDetail(itemKey);
  const now = useNow();
  const [tab, setTab] = useState<Tab>("summary");

  // Opening an item reads it.
  const unread = item?.unread;
  useEffect(() => {
    if (item && unread) void mark(item, "seen");
    // Once per item, as it opens; marks made after are the user's own.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [item?.key, unread === true]);

  const tabs = useMemo(() => {
    if (!item) return [];
    return [
      { id: "summary" as const, label: "Summary", badge: undefined as string | number | undefined },
      ...(hasCode(item)
        ? [{ id: "code" as const, label: "Code", badge: diff?.files.length || undefined }]
        : []),
      ...(hasChecks(item)
        ? [
            {
              id: "checks" as const,
              label: "Checks",
              badge: checks?.some((check) => check.state === "failing") ? "!" : undefined,
            },
          ]
        : []),
    ];
  }, [item, diff, checks]);
  const current = tabs.some((entry) => entry.id === tab) ? tab : "summary";

  const open = () => {
    if (item) void Linking.openURL(item.url);
  };
  const start = (text?: string) =>
    router.push({
      pathname: "/inbox/start",
      params: { key: itemKey, ...(text ? { text } : {}) },
    });

  return (
    <View style={styles.root}>
      <Stack.Screen
        options={{
          title: item ? `${item.repo}${item.number ? ` · ${item.number}` : ""}` : "Inbox",
          headerLeft: () => (
            <HeaderButton icon="close" label="Close" onPress={() => router.back()} />
          ),
          headerRight: () =>
            item ? <HeaderButton icon="external" label="Open in provider" onPress={open} /> : null,
        }}
      />
      <SettingsPage onRefresh={reload} refreshing={false}>
        {!item ? (
          <Missing error={error} loading={loading} onRetry={reload} />
        ) : (
          <>
            <Text style={styles.title}>{item.title}</Text>
            <Facts item={item} detail={detail} />
            <View style={styles.who}>
              <Avatar person={item.author} itemKey={item.key} client={client} />
              <Text style={styles.whoText}>
                <Text style={styles.whoName}>{item.author.name}</Text>
                {` · ${SOURCE_NAMES[item.source]} · updated ${relativeTime(item.updatedAt, now)} ago`}
              </Text>
            </View>
            {tabs.length > 1 ? (
              <View accessibilityRole="tablist" style={styles.tabs}>
                {tabs.map((entry) => (
                  <Pressable
                    key={entry.id}
                    accessibilityRole="tab"
                    accessibilityState={{ selected: entry.id === current }}
                    onPress={() => {
                      void Haptics.selectionAsync();
                      setTab(entry.id);
                    }}
                    style={[styles.tab, entry.id === current && styles.tabOn]}
                  >
                    <Text style={[styles.tabLabel, entry.id === current && styles.tabLabelOn]}>
                      {entry.label}
                    </Text>
                    {entry.badge === "!" ? (
                      <View style={[styles.alertDot, { backgroundColor: colors.deletions }]} />
                    ) : entry.badge ? (
                      <Text style={styles.tabCount}>{entry.badge}</Text>
                    ) : null}
                  </Pressable>
                ))}
              </View>
            ) : null}
            {current === "summary" ? (
              <Summary item={item} detail={detail} loading={loading} client={client} now={now} />
            ) : current === "code" ? (
              <Code
                files={diff?.files ?? null}
                truncated={diff?.truncated ?? false}
                loading={loading}
                itemKey={item.key}
              />
            ) : (
              <Checks
                checks={checks}
                loading={loading}
                onFix={() => start("Fix the failing checks.")}
              />
            )}
            {/* Room for the action bar over the end of the page. */}
            <View style={{ height: 96 + bottom }} />
          </>
        )}
      </SettingsPage>
      <View style={[styles.bar, { paddingBottom: bottom + 12 }]}>
        <Pressable
          accessibilityRole="button"
          accessibilityLabel="Start agent on this"
          disabled={!item}
          onPress={() => {
            void Haptics.impactAsync(Haptics.ImpactFeedbackStyle.Light);
            start();
          }}
          style={({ pressed }) => [styles.primary, !item && styles.disabled, pressed && styles.pressed]}
        >
          <Icon name="sparkle" size={19} color={colors.inverse} />
          <Text style={styles.primaryLabel}>Start agent on this</Text>
        </Pressable>
        <Pressable
          accessibilityRole="button"
          accessibilityLabel="Open in provider"
          disabled={!item}
          onPress={open}
          style={({ pressed }) => [styles.secondary, !item && styles.disabled, pressed && styles.pressed]}
        >
          <Icon name="external" size={19} />
        </Pressable>
      </View>
      <InboxToast bottom={bottom + 84} />
    </View>
  );
}

function Missing({
  error,
  loading,
  onRetry,
}: {
  error: string | null;
  loading: boolean;
  onRetry: () => void;
}) {
  const styles = useStyles();
  const colors = useColors();
  if (loading && !error) return <ActivityIndicator style={styles.center} color={colors.muted} />;
  return (
    <View style={styles.center}>
      <Text style={styles.emptyTitle}>Couldn’t open this item</Text>
      <Text style={styles.muted}>{error ?? "It is no longer in your inbox."}</Text>
      <Pressable accessibilityRole="button" onPress={onRetry} hitSlop={8}>
        <Text style={[styles.link, { color: colors.accent }]}>Try again</Text>
      </Pressable>
    </View>
  );
}

const STATUS_WORDS: Record<InboxItem["status"], string> = {
  open: "Open",
  draft: "Draft",
  closed: "Closed",
  merged: "Merged",
};

function Facts({ item, detail }: { item: InboxItem; detail: InboxDetail | null }) {
  const colors = useColors();
  const styles = useStyles();
  const status =
    item.status === "merged"
      ? colors.accent
      : item.status === "closed"
        ? colors.muted
        : item.status === "draft"
          ? colors.muted
          : colors.additions;
  const decision = detail?.reviewDecision;
  const checks = item.checks ?? detail?.checksSummary;
  return (
    <View style={styles.facts}>
      <Fact tint={status} icon={item.kind === "pr" ? "pullRequest" : "issue"}>
        {STATUS_WORDS[item.status]}
      </Fact>
      {item.attention && item.attention !== "other" ? (
        <Fact tint={item.attention === "ciFailed" ? colors.deletions : colors.caution}>
          {whyLabel(item)}
        </Fact>
      ) : null}
      {decision && decision !== "pending" ? (
        <Fact tint={decision === "approved" ? colors.additions : colors.deletions}>
          {decision === "approved" ? "Approved" : "Changes requested"}
        </Fact>
      ) : null}
      {checks ? (
        <Fact
          tint={
            checks === "passing" ? colors.additions : checks === "failing" ? colors.deletions : colors.caution
          }
        >
          {checks === "passing" ? "Checks passing" : checks === "failing" ? "Checks failing" : "Checks running"}
        </Fact>
      ) : null}
      {item.kind === "pr" && item.additions !== undefined ? (
        <Fact>{`+${item.additions} −${item.deletions ?? 0}`}</Fact>
      ) : null}
    </View>
  );
}

function Fact({
  children,
  tint,
  icon,
}: {
  children: string;
  tint?: string;
  icon?: "pullRequest" | "issue";
}) {
  const colors = useColors();
  const styles = useStyles();
  const color = tint ?? colors.muted;
  return (
    <View style={[styles.fact, { backgroundColor: tint ? withAlpha(tint, 0.16) : colors.fill }]}>
      {icon ? <Icon name={icon} size={14} color={color} /> : null}
      <Text style={[styles.factLabel, { color }]}>{children}</Text>
    </View>
  );
}

function Summary({
  item,
  detail,
  loading,
  client,
  now,
}: {
  item: InboxItem;
  detail: InboxDetail | null;
  loading: boolean;
  client: ReturnType<typeof useInbox>["client"];
  now: number;
}) {
  const styles = useStyles();
  const colors = useColors();
  if (!detail) {
    return loading ? <ActivityIndicator style={styles.center} color={colors.muted} /> : null;
  }
  return (
    <View>
      {detail.body.trim() ? (
        <Markdown text={detail.body} />
      ) : (
        <Text style={styles.muted}>No description.</Text>
      )}
      {detail.thread.length > 0 ? (
        <>
          <Text style={styles.heading}>
            COMMENTS <Text style={styles.headingCount}>{detail.thread.length}</Text>
          </Text>
          {detail.thread.map((entry) => (
            <Comment key={entry.id} entry={entry} itemKey={item.key} client={client} now={now} />
          ))}
        </>
      ) : null}
    </View>
  );
}

function Comment({
  entry,
  itemKey,
  client,
  now,
}: {
  entry: InboxThreadEntry;
  itemKey: string;
  client: ReturnType<typeof useInbox>["client"];
  now: number;
}) {
  const styles = useStyles();
  return (
    <View style={styles.comment}>
      <Avatar person={entry.author} itemKey={itemKey} client={client} size={28} />
      <View style={styles.commentBody}>
        <Text style={styles.commentHead}>
          <Text style={styles.whoName}>{entry.author.name}</Text>
          {entry.kind === "comment" ? "" : ` ${entry.kind === "review" ? "reviewed" : "committed"}`}
          <Text style={styles.commentTime}>{`  ${relativeTime(entry.createdAt, now)}`}</Text>
        </Text>
        <Markdown text={entry.body} />
      </View>
    </View>
  );
}

/** A file of the item's diff as the diff screen reads it. */
export function toChangedFile(file: InboxDiffFile): ChangedFile {
  return {
    relative: file.path,
    status: "modified",
    additions: file.additions,
    deletions: file.deletions,
    lines: file.lines.map((line) => ({
      kind: line.kind,
      text: line.text,
      oldNumber: line.oldNo ?? null,
      newNumber: line.newNo ?? null,
    })),
  };
}

function Code({
  files,
  truncated,
  loading,
  itemKey,
}: {
  files: InboxDiffFile[] | null;
  truncated: boolean;
  loading: boolean;
  itemKey: string;
}) {
  const colors = useColors();
  const styles = useStyles();
  const { client } = useInbox();
  const [opening, setOpening] = useState<string | null>(null);
  if (!files) {
    return loading ? (
      <ActivityIndicator style={styles.center} color={colors.muted} />
    ) : (
      <Text style={styles.muted}>The diff isn’t available.</Text>
    );
  }
  if (files.length === 0) return <Text style={styles.muted}>No changed files.</Text>;

  const open = async (file: InboxDiffFile) => {
    // A file cut short in the list is read whole before it opens.
    let whole = file;
    if (truncated) {
      setOpening(file.path);
      try {
        whole = (await client.diff(itemKey, file.path)).files[0] ?? file;
      } catch {
        // The shorter one still opens.
      } finally {
        setOpening(null);
      }
    }
    router.push({ pathname: "/diff", params: { id: keepDiff(toChangedFile(whole)) } });
  };

  return (
    <View style={styles.card}>
      {files.map((file, index) => {
        const slash = file.path.lastIndexOf("/");
        return (
          <Pressable
            key={file.path}
            accessibilityRole="button"
            accessibilityLabel={`${file.path}, ${file.additions} added, ${file.deletions} removed`}
            onPress={() => void open(file)}
            style={({ pressed }) => [
              styles.fileRow,
              index > 0 && styles.divided,
              pressed && styles.pressed,
            ]}
          >
            <View style={styles.fileName}>
              <Text numberOfLines={1} style={styles.fileTitle}>
                {file.path.slice(slash + 1)}
              </Text>
              <Text numberOfLines={1} style={styles.fileDir}>
                {slash > 0 ? file.path.slice(0, slash) : "/"}
              </Text>
            </View>
            {opening === file.path ? (
              <ActivityIndicator color={colors.muted} />
            ) : (
              <Text style={styles.fileCounts}>
                <Text style={{ color: colors.additions }}>{`+${file.additions}`}</Text>
                <Text style={{ color: colors.deletions }}>{`  −${file.deletions}`}</Text>
              </Text>
            )}
            <Icon name="chevronRight" size={16} color={colors.faint} />
          </Pressable>
        );
      })}
    </View>
  );
}

function Checks({
  checks,
  loading,
  onFix,
}: {
  checks: InboxCheck[] | null;
  loading: boolean;
  onFix: () => void;
}) {
  const colors = useColors();
  const styles = useStyles();
  if (!checks) {
    return loading ? (
      <ActivityIndicator style={styles.center} color={colors.muted} />
    ) : (
      <Text style={styles.muted}>Checks aren’t available.</Text>
    );
  }
  if (checks.length === 0) return <Text style={styles.muted}>No checks ran.</Text>;
  const failing = checks.filter((check) => check.state === "failing").length;
  return (
    <View>
      {failing > 0 ? (
        <View style={[styles.fix, { backgroundColor: withAlpha(colors.deletions, 0.12) }]}>
          <Icon name="failing" size={18} color={colors.deletions} />
          <Text style={[styles.fixText, { color: colors.deletions }]}>
            {failing === 1 ? "1 failing check" : `${failing} failing checks`}
          </Text>
          <Pressable accessibilityRole="button" onPress={onFix} style={styles.fixButton}>
            <Text style={styles.fixButtonLabel}>Fix with agent</Text>
          </Pressable>
        </View>
      ) : null}
      <View style={styles.card}>
        {checks.map((check, index) => (
          <Pressable
            key={`${check.name}:${index}`}
            accessibilityRole={check.url ? "link" : "text"}
            disabled={!check.url}
            onPress={() => check.url && void Linking.openURL(check.url)}
            style={[styles.checkRow, index > 0 && styles.divided]}
          >
            {check.state === "skipped" ? (
              <Icon name="minus" size={18} color={colors.faint} />
            ) : (
              <ChecksGlyph state={check.state} />
            )}
            <Text numberOfLines={1} style={styles.checkName}>
              {check.name}
            </Text>
            <Text style={styles.checkTime}>{checkLabel(check)}</Text>
          </Pressable>
        ))}
      </View>
    </View>
  );
}

/** "failed · 6m 40s", "passed · 41s": a check's outcome and how long it took. */
export function checkLabel(check: InboxCheck) {
  const word =
    check.state === "failing"
      ? "failed"
      : check.state === "pending"
        ? "running"
        : check.state === "skipped"
          ? "skipped"
          : "";
  const time = check.durationMs !== undefined ? duration(check.durationMs) : "";
  return [word, time].filter(Boolean).join(" · ");
}

function duration(ms: number) {
  const seconds = Math.round(ms / 1000);
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  const rest = seconds % 60;
  return rest ? `${minutes}m ${String(rest).padStart(2, "0")}s` : `${minutes}m`;
}

const useStyles = themedStyles((colors) => ({
  root: {
    flex: 1,
  },
  center: {
    alignItems: "center",
    gap: 8,
    paddingTop: 60,
  },
  title: {
    color: colors.content,
    fontSize: 24,
    fontWeight: "700",
    letterSpacing: -0.4,
    lineHeight: 29,
    marginBottom: 12,
  },
  facts: {
    flexDirection: "row",
    flexWrap: "wrap",
    gap: 6,
    marginBottom: 14,
  },
  fact: {
    height: 26,
    flexDirection: "row",
    alignItems: "center",
    gap: 5,
    paddingHorizontal: 10,
    borderRadius: 13,
  },
  factLabel: {
    fontSize: 13,
    fontWeight: "600",
  },
  who: {
    flexDirection: "row",
    alignItems: "center",
    gap: 10,
    marginBottom: 14,
  },
  whoText: {
    flex: 1,
    color: colors.muted,
    fontSize: 14,
  },
  whoName: {
    color: colors.content,
    fontWeight: "600",
  },
  tabs: {
    flexDirection: "row",
    gap: 20,
    marginBottom: 16,
    borderBottomWidth: 1,
    borderBottomColor: colors.stroke,
  },
  tab: {
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
    height: 40,
    marginBottom: -1,
    borderBottomWidth: 2,
    borderBottomColor: "transparent",
  },
  tabOn: {
    borderBottomColor: colors.content,
  },
  tabLabel: {
    color: colors.muted,
    fontSize: 15,
    fontWeight: "600",
  },
  tabLabelOn: {
    color: colors.content,
  },
  tabCount: {
    color: colors.faint,
    fontSize: 12,
  },
  alertDot: {
    width: 8,
    height: 8,
    borderRadius: 4,
  },
  heading: {
    marginTop: 22,
    marginBottom: 10,
    color: colors.muted,
    fontSize: 13,
    fontWeight: "600",
  },
  headingCount: {
    color: colors.faint,
  },
  comment: {
    flexDirection: "row",
    gap: 10,
    marginBottom: 16,
  },
  commentBody: {
    flex: 1,
    gap: 3,
  },
  commentHead: {
    color: colors.content,
    fontSize: 14,
  },
  commentTime: {
    color: colors.faint,
    fontSize: 13,
  },
  muted: {
    color: colors.muted,
    fontSize: 15,
    textAlign: "center",
  },
  emptyTitle: {
    color: colors.content,
    fontSize: 18,
    fontWeight: "600",
  },
  link: {
    fontSize: 15,
    fontWeight: "600",
  },
  card: {
    borderRadius: 14,
    borderCurve: "continuous",
    overflow: "hidden",
    backgroundColor: colors.listCard,
    borderWidth: 1,
    borderColor: colors.stroke,
  },
  divided: {
    borderTopWidth: 1,
    borderTopColor: colors.listDivider,
  },
  fileRow: {
    flexDirection: "row",
    alignItems: "center",
    gap: 10,
    paddingVertical: 12,
    paddingHorizontal: 14,
  },
  fileName: {
    flex: 1,
    minWidth: 0,
  },
  fileTitle: {
    color: colors.content,
    fontSize: 15,
    fontWeight: "600",
  },
  fileDir: {
    color: colors.faint,
    fontSize: 12,
  },
  fileCounts: {
    fontSize: 12,
    fontVariant: ["tabular-nums"],
  },
  checkRow: {
    flexDirection: "row",
    alignItems: "center",
    gap: 10,
    paddingVertical: 12,
    paddingHorizontal: 14,
  },
  checkName: {
    flex: 1,
    color: colors.content,
    fontSize: 15,
    fontWeight: "600",
  },
  checkTime: {
    color: colors.faint,
    fontSize: 13,
  },
  fix: {
    flexDirection: "row",
    alignItems: "center",
    gap: 10,
    marginBottom: 12,
    paddingVertical: 10,
    paddingLeft: 14,
    paddingRight: 12,
    borderRadius: 14,
    borderCurve: "continuous",
  },
  fixText: {
    flex: 1,
    fontSize: 14,
    fontWeight: "600",
  },
  fixButton: {
    height: 32,
    justifyContent: "center",
    paddingHorizontal: 12,
    borderRadius: 16,
    backgroundColor: colors.inverseSurface,
  },
  fixButtonLabel: {
    color: colors.inverse,
    fontSize: 13,
    fontWeight: "700",
  },
  bar: {
    position: "absolute",
    left: 0,
    right: 0,
    bottom: 0,
    flexDirection: "row",
    gap: 10,
    paddingTop: 12,
    paddingHorizontal: 16,
    backgroundColor: withAlpha(colors.sheet, 0.94),
  },
  primary: {
    flex: 1,
    height: 50,
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "center",
    gap: 8,
    borderRadius: 25,
    backgroundColor: colors.inverseSurface,
  },
  primaryLabel: {
    color: colors.inverse,
    fontSize: 17,
    fontWeight: "600",
  },
  secondary: {
    width: 50,
    height: 50,
    alignItems: "center",
    justifyContent: "center",
    borderRadius: 25,
    backgroundColor: colors.fill,
    borderWidth: 1,
    borderColor: colors.stroke,
  },
  disabled: {
    opacity: 0.35,
  },
  pressed: {
    opacity: 0.7,
  },
}));
