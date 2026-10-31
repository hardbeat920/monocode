import { AnimatedLegendList } from "@legendapp/list/reanimated";
import * as Haptics from "expo-haptics";
import { router } from "expo-router";
import { memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import {
  ActivityIndicator,
  Alert,
  Pressable,
  StyleSheet,
  Text,
  View,
} from "react-native";
import Animated, {
  Easing,
  LinearTransition,
  useAnimatedStyle,
  useReducedMotion,
  useSharedValue,
  withTiming,
} from "react-native-reanimated";

import {
  useProjectGit,
  type GitAction,
  type GitStatus,
} from "@/features/connection/useProjectGit";
import { PageBottomBlur, useSessionLayout } from "@/features/sessions/SessionScreen";
import type { ChangedFile, ChangeSummary } from "@/features/workspace/types";
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
/** A file's type icon, between its circle and its name. */
const TYPE_ICON = 18;
/** Every row's height, fixed, so the list places rows without measuring them. */
const FILE_ROW = 64;
const HEADER_ROW = 48;
/** The cards' rounding and the space between them. */
const CARD_RADIUS = 26;
const CARD_GAP = 12;
/** After a circle is tapped, how long the file waits where it is (a second tap
 *  takes it back), then how long it takes to move between the sections. */
const HOLD = 150;
const MOVE = 220;
const EASE = Easing.bezier(0.32, 0.72, 0, 1);
/** How long a staged file is kept where the phone put it while the desktop's
 *  changes still say otherwise. */
const WANTED_FOR = 20_000;
/** After the last tap, how long until the changes are read again: taps in a
 *  row share one read, and each already shows what it did. */
const SETTLE_READ = 1200;

type SectionKey = "staged" | "changes";
type Mark = "on" | "partly" | "off";
/** A row of the virtualized list: the gap between two cards, a section's
 *  header, or one of its files. The cards are drawn by the rows themselves,
 *  rounded at the card's ends, so a working copy with tens of thousands of
 *  changes renders only what is seen. A file moving between sections shows in
 *  both for the move: fading out of the one it leaves, where it takes no room,
 *  and into the one it joins, while the rows between slide by. */
type Row =
  | { kind: "gap" }
  | { kind: "header"; section: SectionKey; count: number; open: boolean }
  | {
      kind: "file";
      section: SectionKey;
      file: ChangedFile;
      first: boolean;
      last: boolean;
      mark: Mark;
      phase?: "enter" | "exit";
    };

const rowKey = (row: Row) =>
  row.kind === "gap"
    ? "gap"
    : row.kind === "header"
      ? `header:${row.section}`
      : `${row.phase === "exit" ? "exit" : row.section}:${row.file.relative}`;

const rowSize = (row: Row) =>
  row.kind === "gap"
    ? CARD_GAP
    : row.kind === "header"
      ? HEADER_ROW
      : row.phase === "exit"
        ? 0
        : FILE_ROW;

const rowType = (row: Row) => row.kind;

const isStaged = (file: ChangedFile) => !!file.staged && !file.unstaged;

/** Where a file sits among others sorted by path, as the desktop sorts them. */
function sortedIndex(files: ChangedFile[], relative: string) {
  let low = 0;
  let high = files.length;
  while (low < high) {
    const mid = (low + high) >> 1;
    if (files[mid].relative < relative) low = mid + 1;
    else high = mid;
  }
  return low;
}

/**
 * The project's uncommitted changes, after the desktop's changes panel and
 * diff view: each file with its status, the branch and totals being on the
 * line under the header. Tapping a file opens its
 * unified diff in a bottom sheet. The circle on a file stages or unstages it,
 * and staged files gather above the rest, as in the desktop's panel. A glass
 * button where the composer rests opens the commit sheet, the desktop's
 * commit box, with the model's grouping of the changes into commits.
 */
export function ChangesView({
  projectId,
  worktreePath,
  active,
  fresh = false,
  files,
  summary,
  hasMore = false,
  onEndReached,
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
  /** The files read so far: staged first, then the rest. */
  files: ChangedFile[];
  /** Every change counted, read or not. */
  summary: ChangeSummary;
  /** More files to read past these, asked for as the list nears its end. */
  hasMore?: boolean;
  onEndReached?: () => void;
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
  // state, kept until the desktop's changes agree with it. A read that began
  // before the staging can arrive after it, and mustn't put the file back.
  // Past WANTED_FOR, the desktop's word stands even if it never agreed.
  const [wanted, setWanted] = useState<Map<string, { staged: boolean; done: boolean }>>(
    new Map(),
  );
  const [shownFiles, setShownFiles] = useState(files);
  if (shownFiles !== files) {
    setShownFiles(files);
    if (wanted.size) {
      const byPath = new Map(files.map((file) => [file.relative, file]));
      setWanted(
        (current) =>
          new Map(
            [...current].filter(([path, entry]) => {
              if (!entry.done) return true;
              const file = byPath.get(path);
              return !!file && isStaged(file) !== entry.staged;
            }),
          ),
      );
    }
  }
  const [stagingAll, setStagingAll] = useState(false);
  // Files whose circle was tapped and are staging, held in place a moment
  // before they move: where they're going.
  const [pending, setPending] = useState<Map<string, boolean>>(new Map());
  // Files moving between the sections, with the section they leave.
  const [moving, setMoving] = useState<Map<string, { from: SectionKey; file: ChangedFile }>>(
    new Map(),
  );
  const timers = useRef(new Map<string, { timer: ReturnType<typeof setTimeout>; toStaged: boolean }>());
  const reduceMotion = useReducedMotion();
  const [collapsed, setCollapsed] = useState<Set<SectionKey>>(new Set());
  const toggleSection = (section: SectionKey) =>
    setCollapsed((current) => {
      const next = new Set(current);
      if (!next.delete(section)) next.add(section);
      return next;
    });

  // With tens of thousands of files, worked out again only when they or the
  // staging change, not on every render.
  const { shown, staged, unstaged, moved } = useMemo(() => {
    const shown = wanted.size
      ? files.map((file) => {
          const entry = wanted.get(file.relative);
          return entry ? { ...file, staged: entry.staged, unstaged: !entry.staged } : file;
        })
      : files;
    // Partly staged files stay with the unstaged ones, their circle half filled.
    // A file just tapped stays in the section it's leaving until it moves.
    const staged: ChangedFile[] = [];
    const unstaged: ChangedFile[] = [];
    for (const file of shown) {
      const held = pending.get(file.relative);
      const toStaged = held !== undefined ? !held : isStaged(file);
      (toStaged ? staged : unstaged).push(file);
    }
    // Staged here but not yet in the desktop's counts, or the other way.
    let moved = 0;
    for (const file of files) {
      const entry = wanted.get(file.relative);
      if (entry && entry.staged !== isStaged(file)) moved += entry.staged ? 1 : -1;
    }
    return { shown, staged, unstaged, moved };
  }, [files, wanted, pending]);
  // The sections' counts: the desktop's, for every file, with what was staged
  // here since. The lists themselves hold only the files read so far.
  const counts = useMemo(
    () => ({ ...summary, staged: Math.max(0, summary.staged + moved) }),
    [summary, moved],
  );
  const bar = fresh ? null : barAction(counts.files, counts.staged, git.status);
  // The composer slides away with the chat page; the commit button takes its
  // place, else only the safe area and its blur are left below.
  const bottom = bar ? dockBottom + BAR + BAR_FADE : bareBottom;

  const changed = () => {
    git.reload();
    onChanged();
  };

  const readTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const readSoon = () => {
    clearTimeout(readTimer.current);
    readTimer.current = setTimeout(onChanged, SETTLE_READ);
  };
  useEffect(() => () => clearTimeout(readTimer.current), []);

  const markWanted = (paths: string[], toStaged: boolean, done: boolean) =>
    setWanted((current) => {
      const next = new Map(current);
      for (const path of paths) next.set(path, { staged: toStaged, done });
      return next;
    });
  const unmarkWanted = (paths: string[]) => {
    const failed = new Set(paths);
    setWanted((current) => new Map([...current].filter(([path]) => !failed.has(path))));
  };
  // Done, though the desktop's word stands after a while even if it never agreed.
  const confirmWanted = (paths: string[], toStaged: boolean) => {
    markWanted(paths, toStaged, true);
    setTimeout(
      () =>
        setWanted((current) => {
          const next = new Map(current);
          for (const path of paths) if (next.get(path)?.done) next.delete(path);
          return next;
        }),
      WANTED_FOR,
    );
  };

  // Staging runs one request at a time: git takes a lock on its index, and two
  // at once fail. Taps made while one runs wait, and go together as the next.
  const queue = useRef<Promise<void>>(Promise.resolve());
  const waiting = useRef(new Map<string, boolean>());
  const enqueue = (job: () => Promise<void>) => {
    const next = queue.current.then(job);
    queue.current = next.catch(() => {});
    return next;
  };
  // Anything else holding the lock, the desktop's own panel say, is waited out.
  const send = async (action: GitAction) => {
    for (let attempt = 1; ; attempt++) {
      try {
        return await git.run(action);
      } catch (reason) {
        const message = reason instanceof Error ? reason.message : String(reason);
        if (attempt >= 4 || !/index\.lock/i.test(message)) throw reason;
        await new Promise((resolve) => setTimeout(resolve, 250 * attempt));
      }
    }
  };

  const flush = async () => {
    const batch = new Map(waiting.current);
    waiting.current.clear();
    if (batch.size === 0) return;
    const failures: string[] = [];
    for (const toStaged of [true, false]) {
      const paths = [...batch].filter(([, to]) => to === toStaged).map(([path]) => path);
      if (paths.length === 0) continue;
      try {
        await send({ action: toStaged ? "stage" : "unstage", paths });
        confirmWanted(paths, toStaged);
      } catch (reason) {
        unmarkWanted(paths);
        failures.push(reason instanceof Error ? reason.message : String(reason));
      }
    }
    if (failures.length) Alert.alert("Couldn't stage every file", failures[0]);
    readSoon();
  };

  /** Shown at once; sent in turn, files tapped close together in one request. */
  const stage = (paths: string[], toStaged: boolean) => {
    markWanted(paths, toStaged, false);
    for (const path of paths) waiting.current.set(path, toStaged);
    return enqueue(flush);
  };

  const stageAll = async (toStaged: boolean) => {
    const paths = (toStaged ? unstaged : staged).map((file) => file.relative);
    if (paths.length === 0) return;
    setStagingAll(true);
    markWanted(paths, toStaged, false);
    await enqueue(async () => {
      try {
        await send({ action: toStaged ? "stageAll" : "unstageAll" });
        confirmWanted(paths, toStaged);
      } catch (reason) {
        unmarkWanted(paths);
        Alert.alert(
          toStaged ? "Couldn't stage" : "Couldn't unstage",
          reason instanceof Error ? reason.message : String(reason),
        );
      }
      onChanged();
    });
    setStagingAll(false);
  };

  const open = (file: ChangedFile) =>
    router.push({ pathname: "/diff", params: { id: keepDiff(file) } });

  const drop =
    <T,>(path: string) =>
    (current: Map<string, T>) => {
      if (!current.has(path)) return current;
      const next = new Map(current);
      next.delete(path);
      return next;
    };

  // Optimistic: the tap is sent at once and shown as done, the circle filling,
  // and the desktop's answer confirms it or puts the file back. The file stays
  // put a moment before it moves, so a second tap can take it back.
  const settle = (file: ChangedFile, toStaged: boolean) => {
    const path = file.relative;
    timers.current.delete(path);
    setPending(drop(path));
    if (!reduceMotion) {
      setMoving((current) =>
        new Map(current).set(path, { from: toStaged ? "changes" : "staged", file }),
      );
      setTimeout(() => setMoving(drop(path)), MOVE);
    }
  };

  const toggle = (file: ChangedFile) => {
    const path = file.relative;
    if (moving.has(path)) return;
    const held = timers.current.get(path);
    if (held) {
      clearTimeout(held.timer);
      timers.current.delete(path);
      setPending(drop(path));
      void stage([path], !held.toStaged);
      return;
    }
    const toStaged = !isStaged(file);
    void Haptics.selectionAsync();
    setPending((current) => new Map(current).set(path, toStaged));
    void stage([path], toStaged);
    timers.current.set(path, {
      toStaged,
      timer: setTimeout(() => handlers.current.settle(file, toStaged), HOLD),
    });
  };

  // The rows and the list keep one identity across renders, so a poll that
  // changes nothing redraws no row; these reach the latest state through a ref.
  const handlers = useRef({ open, toggle, settle, toggleSection, stageAll, stage });
  useLayoutEffect(() => {
    handlers.current = { open, toggle, settle, toggleSection, stageAll, stage };
  });
  const onOpen = useCallback((file: ChangedFile) => handlers.current.open(file), []);
  const onToggle = useCallback((file: ChangedFile) => handlers.current.toggle(file), []);

  useEffect(
    () => () => {
      for (const { timer } of timers.current.values()) clearTimeout(timer);
    },
    [],
  );

  const rows = useMemo(() => {
    const rows: Row[] = [];
    for (const [section, sectionFiles] of [
      ["staged", staged],
      ["changes", unstaged],
    ] as const) {
      // Files moving out of this section, put back where they were to fade.
      let list = sectionFiles;
      const leaving = new Set<string>();
      for (const [path, move] of moving) {
        if (move.from !== section) continue;
        // Back in this section, staging having failed, it needs no copy.
        const at = sortedIndex(list, path);
        if (list[at]?.relative === path) continue;
        if (list === sectionFiles) list = [...sectionFiles];
        list.splice(at, 0, move.file);
        leaving.add(path);
      }
      const count = section === "staged" ? counts.staged : counts.files - counts.staged;
      if (list.length === 0 && count <= 0) continue;
      if (rows.length > 0) rows.push({ kind: "gap" });
      const open = !collapsed.has(section);
      rows.push({ kind: "header", section, count: Math.max(count, sectionFiles.length), open });
      if (!open) continue;
      // The card's bottom is the last file staying in it, rounded at once
      // rather than once the file leaving below it has gone.
      let end = list.length - 1;
      while (end > 0 && leaving.has(list[end].relative)) end--;
      list.forEach((file, index) => {
        const exit = leaving.has(file.relative) && file === moving.get(file.relative)?.file;
        const want = pending.get(file.relative);
        const mark: Mark = exit
          ? section === "changes"
            ? "on"
            : "off"
          : want !== undefined
            ? want
              ? "on"
              : "off"
            : isStaged(file)
              ? "on"
              : file.staged
                ? "partly"
                : "off";
        rows.push({
          kind: "file",
          section,
          file,
          first: index === 0,
          last: index >= end,
          mark,
          phase: exit ? "exit" : moving.has(file.relative) ? "enter" : undefined,
        });
      });
    }
    return rows;
  }, [staged, unstaged, collapsed, moving, pending, counts]);

  const renderRow = useCallback(
    ({ item }: { item: Row }) => {
      if (item.kind === "gap") return <View style={{ height: CARD_GAP }} />;
      if (item.kind === "header") {
        const staged = item.section === "staged";
        return (
          <SectionHeader
            title={staged ? "Staged Changes" : "Changes"}
            count={item.count}
            open={item.open}
            onToggle={() => handlers.current.toggleSection(item.section)}
            action={
              staged
                ? { icon: "minus", label: "Unstage All", onPress: () => void handlers.current.stageAll(false) }
                : { icon: "add", label: "Stage All", onPress: () => void handlers.current.stageAll(true) }
            }
          />
        );
      }
      return (
        <FileRow
          file={item.file}
          first={item.first}
          last={item.last}
          mark={item.mark}
          phase={item.phase}
          onOpen={onOpen}
          onToggle={onToggle}
        />
      );
    },
    [onOpen, onToggle],
  );

  // The other rows slide to where a move puts them, in step with its fades.
  const layout = useMemo(
    () => (reduceMotion ? undefined : LinearTransition.duration(MOVE).easing(EASE)),
    [reduceMotion],
  );

  const contentStyle = [styles.content, { paddingTop: top, paddingBottom: bottom }];
  const listStyle = useMemo(
    () => [styles.content, { paddingTop: top + 4, paddingBottom: bottom }],
    [styles.content, top, bottom],
  );

  return (
    <>
      {files.length === 0 && counts.files === 0 ? (
        // Nothing to list: the Changes card, folded and at zero, rather than a note.
        <View style={[styles.sections, contentStyle, { paddingTop: top + 4 }]}>
          <SectionHeader title="Changes" count={0} loading={loading && !error} />
          {error ? <Text style={styles.empty}>{error}</Text> : null}
        </View>
      ) : (
        <AnimatedLegendList
          data={rows}
          keyExtractor={rowKey}
          renderItem={renderRow}
          // Rows hold no state of their own, so their views can be reused.
          recycleItems
          getItemType={rowType}
          getFixedItemSize={rowSize}
          // Far enough ahead that a fling doesn't outrun the rows.
          drawDistance={1000}
          itemLayoutAnimation={layout}
          // Keeping the top row in place moves the scroll as the rows above
          // change, against their slide; the rows move on their own instead.
          maintainVisibleContentPosition={false}
          // The next page of files, a couple of screens before the end.
          onEndReached={hasMore ? onEndReached : undefined}
          onEndReachedThreshold={2}
          ListFooterComponent={hasMore ? <ActivityIndicator style={styles.more} color={colors.muted} /> : null}
          // The header's gap from the page top, which the empty card gets from `sections`.
          contentContainerStyle={listStyle}
          showsVerticalScrollIndicator={false}
        />
      )}
      {bar ? (
        <>
          <GradualBlur edge="bottom" height={bottom} color={colors.background} />
          <View style={[styles.bar, { bottom: dockBottom }]}>
            <LiquidView
              onPress={() => setSheetOpen(true)}
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
        projectId={projectId}
        worktreePath={worktreePath}
        harness={harness}
        status={git.status}
        error={git.error}
        files={shown}
        summary={counts}
        run={git.run}
        onChanged={changed}
      />
    </>
  );
}

/** The top of a section's card: its header, after the desktop's, with its title
 *  and count, staging or unstaging all of it, and a chevron; tapping it folds
 *  the files away to leave just the header. Its files are the rows after it. */
function SectionHeader({
  title,
  count,
  open = false,
  onToggle,
  action,
  loading = false,
}: {
  title: string;
  count: number;
  /** Without these, as with no changes, the card stays folded and does nothing. */
  open?: boolean;
  onToggle?: () => void;
  action?: { icon: IconName; label: string; onPress: () => void };
  /** Shows a spinner for the count while the changes are first read. */
  loading?: boolean;
}) {
  const colors = useColors();
  const styles = useStyles();
  const empty = count === 0;
  return (
    <View style={[styles.card, open && styles.cardTop]}>
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
    </View>
  );
}

/** What the bottom button offers: committing, by hand what is staged or with
 *  the desktop's model everything, else the branch's next step, as the
 *  desktop's panel leads with it. None with nothing to do. */
function barAction(
  changed: number,
  staged: number,
  status: GitStatus | null,
): { icon: IconName; label: string } | null {
  if (staged > 0) return { icon: "check", label: `Commit ${staged} ${staged === 1 ? "file" : "files"}` };
  if (changed > 0) return { icon: "check", label: "Commit Changes" };
  if (!status?.remote) return null;
  if (!status.upstream) return { icon: "send", label: "Publish Branch" };
  if (status.ahead > 0 || status.behind > 0) return { icon: "send", label: "Sync" };
  if (status.pr) return { icon: "branch", label: `PR #${status.pr.number}` };
  if (status.aheadOfDefault > 0 && status.branch !== status.defaultBranch)
    return { icon: "branch", label: "Create PR" };
  return null;
}

/** A file's row: a slice of its section's card, with the line above it. Fades
 *  in or out while it moves between the sections. */
const FileRow = memo(function FileRow({
  file,
  first,
  last,
  mark,
  phase,
  onOpen,
  onToggle,
}: {
  file: ChangedFile;
  first: boolean;
  last: boolean;
  mark: Mark;
  phase?: "enter" | "exit";
  onOpen: (file: ChangedFile) => void;
  onToggle: (file: ChangedFile) => void;
}) {
  const styles = useStyles();
  // How open the row is. The list places the rows below as if a moving row had
  // already opened or closed, and slides them there with the same timing, so
  // the row grows into the gap they leave, or shrinks as they cover it.
  const open = useSharedValue(phase === "enter" ? 0 : 1);
  // Set before the frame is drawn: a recycled view keeps the last file's value.
  useLayoutEffect(() => {
    const timing = { duration: MOVE, easing: EASE };
    if (phase === "enter") {
      open.set(0);
      open.set(withTiming(1, timing));
    } else if (phase === "exit") {
      open.set(withTiming(0, timing));
    } else {
      open.set(1);
    }
  }, [phase, file.relative, open]);
  const fade = useAnimatedStyle(() => ({
    opacity: open.get(),
    height: open.get() * FILE_ROW,
  }));

  return (
    <Animated.View
      style={[styles.cardRow, last && styles.cardBottom, phase === "exit" && styles.inert, fade]}
    >
      <FileChange file={file} mark={mark} onOpen={onOpen} onToggle={onToggle} />
      <View style={first ? styles.cardDivider : styles.divider} />
    </Animated.View>
  );
});

function FileChange({
  file,
  mark,
  onOpen,
  onToggle,
}: {
  file: ChangedFile;
  mark: Mark;
  onOpen: (file: ChangedFile) => void;
  onToggle: (file: ChangedFile) => void;
}) {
  const colors = useColors();
  const styles = useStyles();
  const { name, dir } = splitPath(file.relative);
  const diff = useDiffTheme();
  // As on desktop, a deleted file has no diff to open.
  const openable = file.lines.length > 0;

  return (
    // Laid out like the SessionCard in the drawer.
    <Pressable
      accessibilityRole="button"
      disabled={!openable}
      onPress={() => onOpen(file)}
      style={({ pressed }) => [styles.fileRow, pressed && styles.fileRowPressed]}
    >
      <Pressable
        accessibilityRole="checkbox"
        accessibilityLabel={mark === "on" ? `Unstage ${name}` : `Stage ${name}`}
        accessibilityState={{ checked: mark === "on" ? true : mark === "partly" ? "mixed" : false }}
        onPress={() => onToggle(file)}
        hitSlop={{ top: 12, bottom: 12, left: 14, right: 8 }}
        style={[styles.check, mark === "on" && styles.checkOn, mark === "partly" && styles.checkPartly]}
      >
        {mark === "on" ? (
          <Icon name="check" size={13} color={colors.inverse} />
        ) : mark === "partly" ? (
          <Icon name="minus" size={13} color={colors.content} />
        ) : null}
      </Pressable>
      {/* The explorer's file-type icon, as on the Files page and desktop,
          centred on the row with the circle rather than on the name's line. */}
      <View style={styles.typeIcon}>
        <FileTypeIcon name={name} isDir={false} size={TYPE_ICON} />
      </View>
      <View style={styles.fileText}>
        <View style={styles.cardLine}>
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
    borderRadius: CARD_RADIUS,
    borderCurve: "continuous",
    backgroundColor: withAlpha(colors.content, 0.05),
    overflow: "hidden",
  },
  // An open card's header: the card goes on in its file rows below.
  cardTop: {
    borderBottomLeftRadius: 0,
    borderBottomRightRadius: 0,
  },
  // A file row, drawn as a slice of its section's card.
  cardRow: {
    backgroundColor: withAlpha(colors.content, 0.05),
    overflow: "hidden",
  },
  more: {
    paddingVertical: 20,
  },
  // A row fading out of the section it left, which takes no taps.
  inert: {
    pointerEvents: "none",
  },
  cardBottom: {
    borderBottomLeftRadius: CARD_RADIUS,
    borderBottomRightRadius: CARD_RADIUS,
    borderCurve: "continuous",
  },
  // Over the row's top edge, so it takes none of the row's fixed height.
  cardDivider: {
    position: "absolute",
    top: 0,
    left: 0,
    right: 0,
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
    height: HEADER_ROW,
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
    position: "absolute",
    top: 0,
    left: INSET + LEAD + LEAD_GAP + TYPE_ICON + LEAD_GAP,
    right: INSET,
    height: StyleSheet.hairlineWidth,
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
    height: FILE_ROW,
    paddingHorizontal: INSET,
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
