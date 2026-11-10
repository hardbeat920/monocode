import { router } from "expo-router";
import { Fragment, useState, type ReactNode } from "react";
import { ActivityIndicator, Alert, Pressable, ScrollView, StyleSheet, Text, View } from "react-native";
import Animated, { FadeIn } from "react-native-reanimated";

import { useProjectGit, type GitStatus } from "@/features/connection/useProjectGit";
import { PageBottomBlur, useSessionLayout } from "@/features/sessions/SessionScreen";
import type { ChangedFile } from "@/features/workspace/types";
import { FileTypeIcon } from "@/features/files/FileTypeIcon";
import { GradualBlur } from "@/ui/GradualBlur";
import { Icon, type IconName } from "@/ui/Icon";
import { LiquidView } from "@/ui/LiquidView";
import { withAlpha, themedStyles, useColors } from "@/ui/theme";

import { CommitSheet } from "./CommitSheet";
import { keepDiff } from "./shownDiffs";
import { formatCount, MONO, splitPath, STATUS_LETTER, useDiffTheme } from "./diffTheme";

/** The commit button: the resting composer's height, so the pages' bottoms match. */
const BAR = 50;
/** How far the blur reaches past the bar. */
const BAR_FADE = 16;
/** The grid every row shares: an inset from the cards' edges, a leading slot
 *  for the branch icon or a file's check circle, and the gap after it, so their
 *  text starts in one column. Section titles start on the leading edge. */
const INSET = 16;
const LEAD = 22;
const LEAD_GAP = 10;
/** A file's type icon before its name. */
const TYPE_ICON = 14;

/**
 * The project's uncommitted changes, after the desktop's changes panel and
 * diff view: each file with its status, the branch and totals being on the
 * line under the header. Tapping a file opens its
 * unified diff in a bottom sheet. The circle on a file stages or unstages it,
 * and staged files gather above the rest, as in the desktop's panel. A glass
 * button where the composer rests stages everything, or with something staged
 * opens the commit sheet, the desktop's commit box.
 */
export function ChangesView({
  projectId,
  worktreePath,
  active,
  fresh = false,
  files,
  loading,
  error,
  onChanged,
  harness,
}: {
  projectId: string;
  worktreePath?: string;
  /** Whether the page is showing, so the branch is only read when looked at. */
  active: boolean;
  /** A new worktree yet to be made: nothing to commit or push. */
  fresh?: boolean;
  files: ChangedFile[];
  loading?: boolean;
  error?: string | null;
  /** After a commit or push, to read the changes again. */
  onChanged: () => void;
  /** The composer's harness, which writes commit messages and pull requests. */
  harness?: string;
}) {
  const colors = useColors();
  const styles = useStyles();
  const { pageTop: top, bareBottom, dockBottom } = useSessionLayout();
  const [sheetOpen, setSheetOpen] = useState(false);
  const git = useProjectGit(
    projectId,
    !fresh && (active || sheetOpen),
    worktreePath,
    harness,
  );
  // Staging shown at once, ahead of the desktop's answer: each file's wanted
  // state, kept until the changes are read again after it is done.
  const [wanted, setWanted] = useState<Map<string, { staged: boolean; done: boolean }>>(new Map());
  const [shownFiles, setShownFiles] = useState(files);
  if (shownFiles !== files) {
    setShownFiles(files);
    setWanted((current) => new Map([...current].filter(([, entry]) => !entry.done)));
  }
  const [stagingAll, setStagingAll] = useState(false);
  const [collapsed, setCollapsed] = useState<Set<"staged" | "changes">>(new Set());
  const toggleSection = (section: "staged" | "changes") =>
    setCollapsed((current) => {
      const next = new Set(current);
      if (!next.delete(section)) next.add(section);
      return next;
    });

  const shown = files.map((file) => {
    const entry = wanted.get(file.relative);
    return entry
      ? { ...file, staged: entry.staged, unstaged: !entry.staged }
      : file;
  });
  // Partly staged files stay with the unstaged ones, their circle half filled.
  const staged = shown.filter((file) => file.staged && !file.unstaged);
  const unstaged = shown.filter((file) => !(file.staged && !file.unstaged));
  const bar = fresh ? null : barAction(files.length, staged.length, git.status);
  // The composer slides away with the chat page; the commit button takes its
  // place, else only the safe area and its blur are left below.
  const bottom = bar ? dockBottom + BAR + BAR_FADE : bareBottom;

  const changed = () => {
    git.reload();
    onChanged();
  };

  const stage = async (paths: string[], toStaged: boolean, all = false) => {
    const mark = (done: boolean) =>
      setWanted((current) => {
        const next = new Map(current);
        for (const path of paths) next.set(path, { staged: toStaged, done });
        return next;
      });
    mark(false);
    try {
      await git.run(
        all
          ? { action: toStaged ? "stageAll" : "unstageAll" }
          : { action: toStaged ? "stage" : "unstage", paths },
      );
      mark(true);
    } catch (reason) {
      setWanted((current) => new Map([...current].filter(([path]) => !paths.includes(path))));
      Alert.alert(
        toStaged ? "Couldn't stage" : "Couldn't unstage",
        reason instanceof Error ? reason.message : String(reason),
      );
    }
    onChanged();
  };

  const stageAll = async (toStaged: boolean) => {
    const paths = (toStaged ? unstaged : staged).map((file) => file.relative);
    if (paths.length === 0) return;
    setStagingAll(true);
    await stage(paths, toStaged, true);
    setStagingAll(false);
  };

  const open = (file: ChangedFile) =>
    router.push({ pathname: "/diff", params: { id: keepDiff(file) } });

  const list = (section: ChangedFile[]) =>
    section.map((file, index) => (
      <Fragment key={file.relative}>
        {index > 0 ? <View style={styles.divider} /> : null}
        <FileChange
          file={file}
          onPress={() => open(file)}
          onToggle={() => void stage([file.relative], !(file.staged && !file.unstaged))}
        />
      </Fragment>
    ));

  return (
    <>
      <ScrollView
        contentContainerStyle={[styles.content, { paddingTop: top, paddingBottom: bottom }]}
        showsVerticalScrollIndicator={false}
      >
        {files.length === 0 ? (
          // Nothing to list: the Changes card, folded and at zero, rather than a note.
          <View style={styles.sections}>
            <Section title="Changes" count={0} loading={loading && !error} />
            {error ? <Text style={styles.empty}>{error}</Text> : null}
          </View>
        ) : (
          <View style={styles.sections}>
            {staged.length > 0 ? (
              <Section
                key="staged"
                title="Staged Changes"
                count={staged.length}
                open={!collapsed.has("staged")}
                onToggle={() => toggleSection("staged")}
                action={{ icon: "minus", label: "Unstage All", onPress: () => void stageAll(false) }}
              >
                {list(staged)}
              </Section>
            ) : null}
            {unstaged.length > 0 ? (
              <Section
                key="changes"
                title="Changes"
                count={unstaged.length}
                open={!collapsed.has("changes")}
                onToggle={() => toggleSection("changes")}
                action={{ icon: "add", label: "Stage All", onPress: () => void stageAll(true) }}
              >
                {list(unstaged)}
              </Section>
            ) : null}
          </View>
        )}
      </ScrollView>
      {bar ? (
        <>
          <GradualBlur edge="bottom" height={bottom} color={colors.background} />
          <View style={[styles.bar, { bottom: dockBottom }]}>
            <LiquidView
              onPress={() => (bar.stage ? void stageAll(true) : setSheetOpen(true))}
              accessibilityLabel={bar.label}
              disabled={stagingAll}
              pressedScale={1.02}
              style={styles.barButton}
              innerStyle={styles.barInner}
            >
              {stagingAll ? (
                <ActivityIndicator color={colors.content} />
              ) : (
                <Icon name={bar.icon} size={18} />
              )}
              <Text style={styles.barLabel}>{bar.label}</Text>
            </LiquidView>
          </View>
        </>
      ) : (
        <PageBottomBlur />
      )}
      <CommitSheet
        isPresented={sheetOpen}
        onDismiss={() => setSheetOpen(false)}
        status={git.status}
        error={git.error}
        staged={staged.length}
        changed={files.length}
        additions={staged.reduce((sum, f) => sum + f.additions, 0)}
        deletions={staged.reduce((sum, f) => sum + f.deletions, 0)}
        run={git.run}
        onChanged={changed}
      />
    </>
  );
}

/** A section as a card: its header, after the desktop's, with its title and
 *  count, staging or unstaging all of it, and a chevron; tapping the header
 *  folds the files away to leave just the header. */
function Section({
  title,
  count,
  open = false,
  onToggle,
  action,
  loading = false,
  children,
}: {
  title: string;
  count: number;
  /** Without these, as with no changes, the card stays folded and does nothing. */
  open?: boolean;
  onToggle?: () => void;
  action?: { icon: IconName; label: string; onPress: () => void };
  /** Shows a spinner for the count while the changes are first read. */
  loading?: boolean;
  children?: ReactNode;
}) {
  const colors = useColors();
  const styles = useStyles();
  const empty = count === 0;
  return (
    <View style={styles.card}>
      <Pressable
        accessibilityRole="button"
        accessibilityState={{ expanded: open, disabled: !onToggle }}
        disabled={!onToggle}
        onPress={onToggle}
        style={({ pressed }) => [styles.section, pressed && styles.fileRowPressed]}
      >
        <View style={styles.sectionHeading}>
          <Text style={styles.sectionTitle} numberOfLines={1}>
            {title}
          </Text>
          {loading ? (
            <ActivityIndicator size="small" color={colors.muted} />
          ) : (
            <View style={[styles.badge, empty && styles.badgeEmpty]}>
              <Text style={[styles.badgeLabel, empty && styles.badgeLabelEmpty]}>{formatCount(count)}</Text>
            </View>
          )}
        </View>
        {action ? (
          <Pressable
            accessibilityRole="button"
            accessibilityLabel={action.label}
            onPress={action.onPress}
            hitSlop={8}
            style={({ pressed }) => [styles.sectionAction, pressed && styles.sectionPressed]}
          >
            <Icon name={action.icon} size={18} color={colors.muted} />
          </Pressable>
        ) : null}
        <View style={styles.chevron}>
          <Icon
            name={open ? "chevronDown" : "chevronRight"}
            size={16}
            color={onToggle ? colors.muted : colors.faint}
          />
        </View>
      </Pressable>
      {open ? (
        <Animated.View entering={FadeIn.duration(180)}>
          <View style={styles.cardDivider} />
          {children}
        </Animated.View>
      ) : null}
    </View>
  );
}

/** What the bottom button offers: staging everything while nothing is, then
 *  committing what is, else the branch's next step, as the desktop's panel
 *  leads with it. None with nothing to do. */
function barAction(
  changed: number,
  staged: number,
  status: GitStatus | null,
): { icon: IconName; label: string; stage?: boolean } | null {
  if (staged > 0) return { icon: "check", label: `Commit ${staged} ${staged === 1 ? "file" : "files"}` };
  if (changed > 0) return { icon: "add", label: "Stage All", stage: true };
  if (!status?.remote) return null;
  if (!status.upstream) return { icon: "send", label: "Publish Branch" };
  if (status.ahead > 0 || status.behind > 0) return { icon: "send", label: "Sync" };
  if (status.pr) return { icon: "branch", label: `PR #${status.pr.number}` };
  if (status.aheadOfDefault > 0 && status.branch !== status.defaultBranch)
    return { icon: "branch", label: "Create PR" };
  return null;
}

function FileChange({
  file,
  onPress,
  onToggle,
}: {
  file: ChangedFile;
  onPress: () => void;
  onToggle: () => void;
}) {
  const colors = useColors();
  const styles = useStyles();
  const { name, dir } = splitPath(file.relative);
  const staged = !!file.staged && !file.unstaged;
  const partly = !!file.staged && !!file.unstaged;
  const diff = useDiffTheme();
  // As on desktop, a deleted file has no diff to open.
  const openable = file.lines.length > 0;

  return (
    // Laid out like the SessionCard in the drawer.
    <Pressable
      accessibilityRole="button"
      disabled={!openable}
      onPress={onPress}
      style={({ pressed }) => [styles.fileRow, pressed && styles.fileRowPressed]}
    >
      <Pressable
        accessibilityRole="checkbox"
        accessibilityLabel={staged ? `Unstage ${name}` : `Stage ${name}`}
        accessibilityState={{ checked: staged ? true : partly ? "mixed" : false }}
        onPress={onToggle}
        hitSlop={{ top: 12, bottom: 12, left: 14, right: 8 }}
        style={[styles.check, staged && styles.checkOn, partly && styles.checkPartly]}
      >
        {staged ? (
          <Icon name="check" size={13} color={colors.inverse} />
        ) : partly ? (
          <Icon name="minus" size={13} color={colors.content} />
        ) : null}
      </Pressable>
      <View style={styles.fileText}>
        <View style={styles.cardLine}>
          {/* The explorer's file-type icon, as on the Files page and desktop. */}
          <View style={styles.typeIcon}>
            <FileTypeIcon name={name} isDir={false} size={TYPE_ICON} />
          </View>
          <Text style={styles.fileName} numberOfLines={1}>
            {name}
          </Text>
          {file.additions > 0 ? (
            <Text style={[styles.count, { color: diff.add.fg }]}>+{formatCount(file.additions)}</Text>
          ) : null}
          {file.deletions > 0 ? (
            <Text style={[styles.count, { color: diff.del.fg }]}>-{formatCount(file.deletions)}</Text>
          ) : null}
        </View>
        <View style={styles.cardLine}>
          <View style={styles.dirRow}>
            <View style={styles.typeIcon}>
              <Icon name="folder" size={12} color={colors.faint} />
            </View>
            <Text style={styles.dir} numberOfLines={1}>
              {dir || "/"}
            </Text>
          </View>
          <Text style={[styles.status, { color: diff.status[file.status] }]}>
            {STATUS_LETTER[file.status]}
          </Text>
        </View>
      </View>
    </Pressable>
  );
}

const useStyles = themedStyles((colors) => ({
  sections: {
    gap: 12,
    paddingTop: 4,
  },
  // Cards over the page: a light wash, as the sheets' cards.
  // Rounded as the Files page's native card.
  card: {
    borderRadius: 26,
    borderCurve: "continuous",
    backgroundColor: withAlpha(colors.content, 0.05),
    overflow: "hidden",
  },
  cardDivider: {
    height: StyleSheet.hairlineWidth,
    backgroundColor: colors.stroke,
  },
  // A square slot, so either chevron sits centred on the header's middle.
  chevron: {
    width: 22,
    height: 22,
    alignItems: "center",
    justifyContent: "center",
  },
  section: {
    flexDirection: "row",
    alignItems: "center",
    gap: 4,
    height: 48,
    paddingHorizontal: INSET,
  },
  sectionHeading: {
    flex: 1,
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
  },
  sectionTitle: {
    flexShrink: 1,
    color: colors.muted,
    fontSize: 13,
    fontWeight: "600",
    textTransform: "uppercase",
    letterSpacing: 0.6,
  },
  badge: {
    minWidth: 24,
    height: 20,
    paddingHorizontal: 7,
    borderRadius: 10,
    alignItems: "center",
    justifyContent: "center",
    backgroundColor: colors.accent,
  },
  // At zero the count goes grey, as there is nothing in it to call out.
  badgeEmpty: {
    backgroundColor: colors.surfaceRaised,
  },
  badgeLabelEmpty: {
    color: colors.muted,
  },
  badgeLabel: {
    color: "#ffffff",
    fontSize: 12,
    fontWeight: "600",
    fontVariant: ["tabular-nums"],
  },
  sectionAction: {
    width: 36,
    height: 36,
    alignItems: "center",
    justifyContent: "center",
  },
  sectionPressed: {
    opacity: 0.5,
  },
  check: {
    width: LEAD,
    height: LEAD,
    borderRadius: LEAD / 2,
    borderWidth: 1.5,
    borderColor: colors.faint,
    alignItems: "center",
    justifyContent: "center",
  },
  checkOn: {
    borderColor: colors.inverseSurface,
    backgroundColor: colors.inverseSurface,
  },
  checkPartly: {
    borderColor: colors.muted,
  },
  bar: {
    position: "absolute",
    left: 0,
    right: 0,
  },
  // Inset like the resting composer pill.
  barButton: {
    marginHorizontal: 20,
  },
  // Nudged left, as the commit sheet's buttons, so icon and label look centred.
  barInner: {
    height: BAR,
    justifyContent: "center",
    gap: 8,
    paddingRight: 12,
  },
  barLabel: {
    color: colors.content,
    fontSize: 17,
    fontWeight: "600",
  },
  // The cards' margin from the screen edges; the Files card matches it.
  content: {
    paddingHorizontal: 10,
  },
  // Inset to the text column, as iOS lists inset their separators.
  divider: {
    height: StyleSheet.hairlineWidth,
    marginLeft: INSET + LEAD + LEAD_GAP,
    marginRight: INSET,
    marginVertical: 2,
    backgroundColor: colors.stroke,
  },
  empty: {
    paddingHorizontal: INSET,
    color: colors.muted,
    fontSize: 15,
  },
  // Square: the card's rounding clips its first and last rows.
  fileRow: {
    flexDirection: "row",
    alignItems: "center",
    gap: LEAD_GAP,
    paddingHorizontal: INSET,
    paddingVertical: 12,
  },
  fileText: {
    flex: 1,
    gap: 4,
  },
  cardLine: {
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
  },
  // The folder sits in the same slot as the file icon above it, so the
  // path starts under the name.
  dirRow: {
    flex: 1,
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
  },
  typeIcon: {
    width: TYPE_ICON,
    alignItems: "center",
  },
  fileRowPressed: {
    backgroundColor: withAlpha(colors.content, 0.05),
  },
  fileName: {
    flex: 1,
    color: colors.content,
    fontSize: 15,
    fontWeight: "600",
  },
  dir: {
    flexShrink: 1,
    color: colors.muted,
    fontSize: 12,
  },
  count: {
    fontSize: 13,
    fontWeight: "600",
    fontVariant: ["tabular-nums"],
  },
  status: {
    width: 16,
    textAlign: "right",
    fontFamily: MONO,
    fontSize: 12,
    fontWeight: "600",
  },
}));
