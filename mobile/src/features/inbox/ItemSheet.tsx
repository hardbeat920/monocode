import * as Clipboard from "expo-clipboard";
import * as Haptics from "expo-haptics";
import { router, Stack } from "expo-router";
import { useEffect, useMemo, useState } from "react";
import { ActivityIndicator, Alert, Linking, Pressable, Text, View } from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";

import { MONO } from "@/features/changes/diffTheme";
import { keepDiff } from "@/features/changes/shownDiffs";
import { HeaderButton } from "@/features/files/codeScreen";
import { SettingsPage } from "@/features/settings/SettingsUI";
import { relativeTime } from "@/features/workspace/relativeTime";
import { ChoiceMenu } from "@/ui/ChoiceMenu";
import { GradualBlur } from "@/ui/GradualBlur";
import { Icon, type IconName } from "@/ui/Icon";
import { LiquidView } from "@/ui/LiquidView";
import { themedStyles, useColors, withAlpha } from "@/ui/theme";

import { Avatar } from "./Avatar";
import { hasChecks, hasCode, reloadInboxDetail, useInboxDetail } from "./detail";
import { InboxMarkdown } from "./InboxMarkdown";
import { actionCopy, mergeOptions } from "./itemActions";
import { countComments, lineLabel, useReviewComments } from "./review";
import { ItemMenu, type ItemMenuEntry } from "./ItemMenu";
import { useInbox } from "./InboxProvider";
import { ChecksGlyph, whyLabel } from "./InboxRow";
import { itemState } from "./ItemMark";
import { InboxToast } from "./InboxToast";
import { SOURCE_NAMES } from "./ProviderMark";
import type {
  InboxAction,
  InboxCheck,
  InboxDetail,
  InboxDiffFile,
  InboxItem,
  InboxThreadEntry,
} from "./protocol";
import { checkLabel, toChangedFile } from "./itemView";
import { useNow } from "./useNow";

type Tab = "summary" | "code" | "checks";

/** The action bar over the page's end: the button, and room above and below it. */
const BAR = 86;
const BUTTON = 50;

/**
 * An inbox item's sheet, as the mock lays it out: provider, number, status
 * and author; the description and comments; for a pull request its changed
 * files and, on GitHub, its checks. The action bar starts an agent on it and
 * holds what the desktop's detail offers: merging, the ⋯ menu's other pull
 * request actions, commenting and copying. Anything that reaches the provider
 * is confirmed first. The header's button opens it in the provider's own app
 * or site.
 */
export function ItemSheet({ itemKey }: { itemKey: string }) {
  const colors = useColors();
  const styles = useStyles();
  const { bottom } = useSafeAreaInsets();
  const { client, review, mark, act, notify } = useInbox();
  const { item, detail, diff, checks, error, loading, reload, adopt } = useInboxDetail(itemKey);
  const now = useNow();
  const [tab, setTab] = useState<Tab>("summary");
  const [acting, setActing] = useState(false);

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

  const actions = detail?.actions ?? [];
  const canComment = actions.includes("comment");
  const canReview = actions.includes("review");
  const merges = mergeOptions(actions);
  // The list may have changed at the desk; the Code tab shows it as it stands.
  useEffect(() => {
    if (canReview && current === "code") void review.load(itemKey).catch(() => {});
  }, [canReview, current, itemKey, review]);

  const comment = (replyTo?: string, author?: string) =>
    router.push({
      pathname: "/inbox/comment",
      params: {
        key: itemKey,
        ...(replyTo ? { replyTo } : {}),
        ...(author ? { author } : {}),
      },
    });
  const copy = (text: string, message: string) => {
    void Clipboard.setStringAsync(text);
    void Haptics.selectionAsync();
    notify(message);
  };

  const run = async (action: InboxAction) => {
    if (!item) return;
    setActing(true);
    try {
      adopt(await act(item, action));
      void Haptics.notificationAsync(Haptics.NotificationFeedbackType.Success);
    } catch (reason) {
      Alert.alert(
        actionCopy(action, detail ?? {}).failed,
        reason instanceof Error ? reason.message : String(reason),
      );
    } finally {
      setActing(false);
    }
  };
  const confirm = (action: InboxAction) => {
    if (acting) return;
    const words = actionCopy(action, detail ?? {});
    Alert.alert(words.title, words.detail, [
      { text: "Cancel", style: "cancel" },
      {
        text: words.confirm,
        style: action === "close" ? "destructive" : "default",
        onPress: () => void run(action),
      },
    ]);
  };

  const menu: ItemMenuEntry[] = [];
  if (item) {
    const headRef = detail?.headRef;
    if (canComment) {
      menu.push({
        label: "Add comment",
        icon: "comment",
        symbol: "text.bubble",
        onPress: () => comment(),
      });
    }
    menu.push({
      label: "Copy link",
      icon: "link",
      symbol: "link",
      onPress: () => copy(item.url, "Link copied"),
    });
    if (headRef) {
      menu.push({
        label: "Copy branch name",
        icon: "branch",
        symbol: "arrow.triangle.branch",
        onPress: () => copy(headRef, "Branch name copied"),
      });
    }
    if (actions.includes("ready")) {
      menu.push({
        label: "Ready for review",
        icon: "pullRequest",
        symbol: "checkmark.circle",
        onPress: () => confirm("ready"),
      });
    }
    if (actions.includes("draft")) {
      menu.push({
        label: "Convert to draft",
        icon: "draft",
        symbol: "circle.dashed",
        onPress: () => confirm("draft"),
      });
    }
    if (actions.includes("reopen")) {
      menu.push({
        label: "Reopen pull request",
        icon: "pullRequest",
        symbol: "arrow.uturn.backward",
        onPress: () => confirm("reopen"),
      });
    }
    if (actions.includes("close")) {
      menu.push({
        label: "Close pull request",
        icon: "close",
        symbol: "xmark.circle",
        destructive: true,
        onPress: () => confirm("close"),
      });
    }
  }

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
      {/* No pull to refresh: pulling down at the top is how the sheet is dismissed. */}
      <SettingsPage>
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
              <Summary
                item={item}
                detail={detail}
                loading={loading}
                client={client}
                now={now}
                onComment={canComment ? comment : undefined}
              />
            ) : current === "code" ? (
              <Code
                files={diff?.files ?? null}
                truncated={diff?.truncated ?? false}
                loading={loading}
                itemKey={item.key}
                reviewing={canReview}
              />
            ) : (
              <Checks
                checks={checks}
                loading={loading}
                onFix={() => start("Fix the failing checks.")}
              />
            )}
            {/* Room for the action bar over the end of the page. */}
            <View style={{ height: BAR + bottom }} />
          </>
        )}
      </SettingsPage>
      <GradualBlur edge="bottom" height={BAR + bottom} color={colors.sheet} />
      <View style={[styles.bar, { paddingBottom: bottom + 12 }]}>
        {menu.length > 0 ? <ItemMenu entries={menu} size={BUTTON} /> : null}
        {/* LiquidView's `style` lands inside its Pressable, so the row's flex goes here. */}
        <View style={styles.action}>
          <LiquidView
            accessibilityLabel="Start agent on this"
            disabled={!item}
            onPress={() => start()}
            pressedScale={1.03}
            tintColor={merges.length > 0 ? undefined : colors.inverseSurface}
            style={!item && styles.disabled}
            innerStyle={styles.face}
          >
            <Icon
              name="sparkle"
              size={19}
              color={merges.length > 0 ? colors.content : colors.inverse}
            />
            <Text style={[styles.primaryLabel, merges.length > 0 && styles.secondaryLabel]}>
              {merges.length > 0 ? "Start agent" : "Start agent on this"}
            </Text>
          </LiquidView>
        </View>
        {merges.length > 0 ? (
          <ChoiceMenu
            label="Merge pull request"
            title="Merge pull request"
            disabled={acting}
            actions={merges.map(({ action, label }) => ({
              label,
              onPress: () => confirm(action),
            }))}
            style={styles.action}
          >
            <LiquidView
              interactive={!acting}
              tintColor={colors.inverseSurface}
              style={acting && styles.disabled}
              innerStyle={styles.face}
            >
              {acting ? (
                <ActivityIndicator color={colors.inverse} />
              ) : (
                <>
                  <Icon name="merge" size={19} color={colors.inverse} />
                  <Text style={styles.primaryLabel}>Merge</Text>
                </>
              )}
            </LiquidView>
          </ChoiceMenu>
        ) : null}
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
  const state = itemState(item, colors);
  const decision = detail?.reviewDecision;
  const checks = item.checks ?? detail?.checksSummary;
  return (
    <View style={styles.facts}>
      <Fact tint={state.tint} icon={state.icon}>
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
  icon?: IconName;
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
  onComment,
}: {
  item: InboxItem;
  detail: InboxDetail | null;
  loading: boolean;
  client: ReturnType<typeof useInbox>["client"];
  now: number;
  /** Writes a comment, or with a target a reply. Left out where the desktop can't post. */
  onComment?: (replyTo?: string, author?: string) => void;
}) {
  const styles = useStyles();
  const colors = useColors();
  if (!detail) {
    return loading ? <ActivityIndicator style={styles.center} color={colors.muted} /> : null;
  }
  const comments = detail.thread.filter((entry) => entry.kind !== "commit").length;
  return (
    <View>
      {detail.body.trim() ? (
        <InboxMarkdown text={detail.body} place={{ itemKey: item.key }} client={client} />
      ) : (
        <Text style={styles.muted}>No description.</Text>
      )}
      <View accessibilityRole="header" style={styles.commentsHead}>
        <Icon name="comment" size={20} color={colors.content} />
        <Text style={styles.commentsTitle}>Comments</Text>
        <View style={styles.commentsCount}>
          <Text style={styles.commentsCountLabel}>{comments}</Text>
        </View>
      </View>
      {comments === 0 ? <Text style={styles.noComments}>No comments yet.</Text> : null}
      {detail.thread.map((entry) =>
        entry.kind === "commit" ? (
          <CommitRow key={entry.id} entry={entry} now={now} />
        ) : (
          <Comment
            key={entry.id}
            entry={entry}
            itemKey={item.key}
            client={client}
            now={now}
            onReply={onComment}
          />
        ),
      )}
      {onComment ? (
        <Pressable
          accessibilityRole="button"
          onPress={() => onComment()}
          style={({ pressed }) => [styles.addComment, pressed && styles.pressed]}
        >
          <Icon name="comment" size={18} color={colors.muted} />
          <Text style={styles.addCommentLabel}>Add a comment</Text>
        </Pressable>
      ) : null}
    </View>
  );
}

function Comment({
  entry,
  itemKey,
  client,
  now,
  onReply,
}: {
  entry: InboxThreadEntry;
  itemKey: string;
  client: ReturnType<typeof useInbox>["client"];
  now: number;
  onReply?: (replyTo: string, author: string) => void;
}) {
  const styles = useStyles();
  const { replyTo } = entry;
  return (
    <View style={styles.comment}>
      <View style={styles.commentHead}>
        <Avatar person={entry.author} itemKey={itemKey} client={client} size={28} />
        <Text numberOfLines={1} style={styles.commentWho}>
          <Text style={styles.whoName}>{entry.author.name}</Text>
          {entry.kind === "review" ? " reviewed" : ""}
        </Text>
        <Text style={styles.commentTime}>{relativeTime(entry.createdAt, now)}</Text>
      </View>
      {entry.path ? (
        <Text numberOfLines={1} style={styles.commentPlace}>
          {entry.line ? `${entry.path}:${entry.line}` : entry.path}
        </Text>
      ) : null}
      <InboxMarkdown
        text={entry.body}
        place={{ itemKey, commentId: entry.id }}
        client={client}
      />
      {onReply && replyTo ? (
        <Pressable
          accessibilityRole="button"
          accessibilityLabel={`Reply to ${entry.author.name}`}
          hitSlop={8}
          onPress={() => onReply(replyTo, entry.author.name)}
          style={styles.reply}
        >
          <Text style={styles.replyLabel}>Reply</Text>
        </Pressable>
      ) : null}
    </View>
  );
}

/** A commit between the comments: activity, so it doesn't read as one. */
function CommitRow({ entry, now }: { entry: InboxThreadEntry; now: number }) {
  const colors = useColors();
  const styles = useStyles();
  return (
    <View style={styles.commit}>
      <Icon name="branch" size={14} color={colors.faint} />
      <Text numberOfLines={1} style={styles.commitText}>
        {`${entry.author.name} committed: ${entry.body}`}
      </Text>
      <Text style={styles.commentTime}>{relativeTime(entry.createdAt, now)}</Text>
    </View>
  );
}

/** The line comments waiting to go out, and the button that posts them together. */
function ReviewCard({ itemKey }: { itemKey: string }) {
  const colors = useColors();
  const styles = useStyles();
  const { review, notify } = useInbox();
  const comments = useReviewComments(review, itemKey);
  const [posting, setPosting] = useState(false);

  if (comments.length === 0) {
    return <Text style={styles.reviewHint}>Tap a line in a file to comment on it.</Text>;
  }

  const refused = (title: string, reason: unknown) =>
    Alert.alert(title, reason instanceof Error ? reason.message : String(reason));
  const post = async () => {
    setPosting(true);
    try {
      await review.submit(itemKey);
      void Haptics.notificationAsync(Haptics.NotificationFeedbackType.Success);
      reloadInboxDetail(itemKey);
      notify("Review posted");
    } catch (reason) {
      refused("Couldn’t post the review", reason);
    } finally {
      setPosting(false);
    }
  };
  const confirm = () =>
    Alert.alert(
      "Post this review?",
      `${countComments(comments.length)} will go to GitHub as one review. It doesn’t approve or request changes.`,
      [
        { text: "Cancel", style: "cancel" },
        { text: "Post review", onPress: () => void post() },
      ],
    );

  return (
    <View style={[styles.card, styles.review]}>
      <Text style={styles.reviewTitle}>Your review</Text>
      {comments.map((comment) => (
        <View key={comment.id} style={styles.pending}>
          <View style={styles.pendingBody}>
            <Text numberOfLines={1} style={styles.commentPlace}>
              {lineLabel(comment)}
            </Text>
            <Text numberOfLines={3} style={styles.pendingText}>
              {comment.text}
            </Text>
          </View>
          <Pressable
            accessibilityRole="button"
            accessibilityLabel={`Remove comment on ${lineLabel(comment)}`}
            disabled={posting}
            hitSlop={8}
            onPress={() =>
              void review
                .remove(itemKey, comment.id)
                .catch((reason: unknown) => refused("Couldn’t remove the comment", reason))
            }
          >
            <Icon name="close" size={18} color={colors.muted} />
          </Pressable>
        </View>
      ))}
      <Pressable
        accessibilityRole="button"
        accessibilityLabel="Post review"
        accessibilityState={{ disabled: posting, busy: posting }}
        disabled={posting}
        onPress={confirm}
        style={({ pressed }) => [styles.postReview, pressed && styles.pressed]}
      >
        {posting ? (
          <ActivityIndicator color={colors.inverse} />
        ) : (
          <Text style={styles.postReviewLabel}>Post review</Text>
        )}
      </Pressable>
    </View>
  );
}

function Code({
  files,
  truncated,
  loading,
  itemKey,
  reviewing,
}: {
  files: InboxDiffFile[] | null;
  truncated: boolean;
  loading: boolean;
  itemKey: string;
  /** Its lines take review comments. */
  reviewing: boolean;
}) {
  const colors = useColors();
  const styles = useStyles();
  const { client, review } = useInbox();
  const pending = useReviewComments(review, itemKey);
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
    const id = keepDiff(toChangedFile(whole), reviewing ? itemKey : undefined);
    router.push({ pathname: "/diff", params: { id } });
  };

  return (
    <View>
      {reviewing ? <ReviewCard itemKey={itemKey} /> : null}
      <View style={styles.card}>
        {files.map((file, index) => {
          const slash = file.path.lastIndexOf("/");
          const notes = pending.filter((comment) => comment.path === file.path).length;
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
              {notes > 0 ? (
                <View style={styles.fileNotes}>
                  <Icon name="comment" size={13} color={colors.accent} />
                  <Text style={[styles.fileNotesLabel, { color: colors.accent }]}>{notes}</Text>
                </View>
              ) : null}
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
  commentsHead: {
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
    marginTop: 28,
    marginBottom: 14,
    paddingTop: 20,
    borderTopWidth: 1,
    borderTopColor: colors.stroke,
  },
  commentsTitle: {
    color: colors.content,
    fontSize: 20,
    fontWeight: "700",
    letterSpacing: -0.2,
  },
  commentsCount: {
    minWidth: 24,
    height: 24,
    alignItems: "center",
    justifyContent: "center",
    paddingHorizontal: 7,
    borderRadius: 12,
    backgroundColor: colors.fill,
  },
  commentsCountLabel: {
    color: colors.muted,
    fontSize: 13,
    fontWeight: "700",
    fontVariant: ["tabular-nums"],
  },
  noComments: {
    marginBottom: 8,
    color: colors.muted,
    fontSize: 15,
  },
  comment: {
    gap: 8,
    marginBottom: 10,
    padding: 12,
    borderRadius: 14,
    borderCurve: "continuous",
    backgroundColor: colors.listCard,
    borderWidth: 1,
    borderColor: colors.stroke,
  },
  commentHead: {
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
  },
  commentWho: {
    flex: 1,
    color: colors.muted,
    fontSize: 14,
  },
  commentTime: {
    color: colors.faint,
    fontSize: 13,
  },
  commentPlace: {
    color: colors.muted,
    fontFamily: MONO,
    fontSize: 12,
  },
  reply: {
    alignSelf: "flex-start",
    marginTop: 4,
  },
  replyLabel: {
    color: colors.muted,
    fontSize: 13,
    fontWeight: "600",
  },
  addComment: {
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
    height: 44,
    marginTop: 8,
    paddingHorizontal: 14,
    borderRadius: 14,
    borderCurve: "continuous",
    borderWidth: 1,
    borderColor: colors.stroke,
  },
  addCommentLabel: {
    color: colors.muted,
    fontSize: 15,
  },
  commit: {
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
    marginBottom: 10,
    paddingHorizontal: 4,
  },
  commitText: {
    flex: 1,
    color: colors.muted,
    fontSize: 13,
  },
  review: {
    gap: 10,
    marginBottom: 12,
    padding: 14,
  },
  reviewTitle: {
    color: colors.content,
    fontSize: 15,
    fontWeight: "700",
  },
  reviewHint: {
    marginBottom: 10,
    color: colors.muted,
    fontSize: 13,
  },
  pending: {
    flexDirection: "row",
    alignItems: "flex-start",
    gap: 10,
  },
  pendingBody: {
    flex: 1,
    minWidth: 0,
    gap: 2,
  },
  pendingText: {
    color: colors.content,
    fontSize: 14,
  },
  postReview: {
    height: 44,
    alignItems: "center",
    justifyContent: "center",
    borderRadius: 22,
    backgroundColor: colors.inverseSurface,
  },
  postReviewLabel: {
    color: colors.inverse,
    fontSize: 15,
    fontWeight: "600",
  },
  fileNotes: {
    flexDirection: "row",
    alignItems: "center",
    gap: 3,
  },
  fileNotesLabel: {
    fontSize: 12,
    fontWeight: "700",
    fontVariant: ["tabular-nums"],
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
    alignItems: "center",
    gap: 10,
    paddingTop: 12,
    paddingHorizontal: 16,
  },
  action: {
    flex: 1,
  },
  face: {
    height: BUTTON,
    gap: 8,
  },
  primaryLabel: {
    color: colors.inverse,
    fontSize: 17,
    fontWeight: "600",
  },
  secondaryLabel: {
    color: colors.content,
  },
  disabled: {
    opacity: 0.35,
  },
  pressed: {
    opacity: 0.7,
  },
}));
