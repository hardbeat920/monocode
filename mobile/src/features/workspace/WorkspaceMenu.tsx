import {
  BottomSheet,
  Button,
  Group,
  HStack,
  Host,
  Image,
  List,
  NavigationDestination,
  NavigationLink,
  NavigationStack,
  ProgressView,
  Section,
  Spacer,
  Text,
  VStack,
} from "@expo/ui/swift-ui";
import {
  buttonStyle,
  containerBackground,
  contentShape,
  disabled,
  font,
  foregroundStyle,
  frame,
  lineLimit,
  listSectionSpacing,
  listStyle,
  monospacedDigit,
  navigationBarTitleDisplayMode,
  navigationTitle,
  presentationDetents,
  presentationDragIndicator,
  scrollContentBackground,
  shapes,
  truncationMode,
} from "@expo/ui/swift-ui/modifiers";
import { useRef, useState, type ReactNode } from "react";
import { Pressable, Text as RNText, View } from "react-native";

import { formatCount, useDiffTheme } from "@/features/changes/diffTheme";
import { Icon } from "@/ui/Icon";
import { themedStyles, useColors } from "@/ui/theme";

import type { ChangedFile, Worktree } from "./types";

/** Where a new session works, as the desktop composer's workspace picker offers. */
export type Workspace =
  { mode: "current" } | { mode: "worktree" } | { mode: "existing"; worktree: Worktree };

type RowSymbol = "folder" | "arrow.triangle.branch" | "folder.badge.gearshape" | "plus.forwardslash.minus";

const SECONDARY = foregroundStyle({ type: "hierarchical", style: "secondary" });
/** The pushed page of existing worktrees, by its `NavigationLink` value. */
const EXISTING = "existing";

/** One key per option: its mode, or an existing worktree's path. */
const keyOf = (workspace: Workspace) =>
  workspace.mode === "existing" ? workspace.worktree.path : workspace.mode;

/**
 * The line under an empty session's header: the working copy's branch and how
 * much has changed in it, centred, which stays in view across its pages.
 * Tapping it opens a native bottom sheet with the project's checkout and a new
 * worktree to start the session in, and a page of the worktrees it already has.
 * A pick spins in its row while the line reads the new working copy, and the
 * sheet closes once it has, so the line is current when it shows again.
 */
export function WorkspaceMenu({
  value,
  worktrees,
  onChange,
  branch,
  checkoutBranch,
  files,
  loading,
  onOpenChanges,
}: {
  value: Workspace;
  worktrees: Worktree[];
  onChange: (value: Workspace) => void;
  branch: string;
  /** The project checkout's own branch, to name it in the sheet. */
  checkoutBranch?: string;
  files: ChangedFile[];
  /** The picked working copy's branch and changes are still being read. */
  loading: boolean;
  onOpenChanges: () => void;
}) {
  const colors = useColors();
  const styles = useStyles();
  const [open, setOpen] = useState(false);
  // The option just picked, spinning until the line has read it.
  const [pending, setPending] = useState<string | null>(null);
  // Read: the sheet closes on the line it was for. A pick and the read it
  // starts land in the same render, so this waits for the read to finish.
  if (pending && !loading) {
    setPending(null);
    setOpen(false);
  }
  const dismiss = () => {
    setPending(null);
    setOpen(false);
  };
  return (
    <>
      <Pressable
        accessibilityRole="button"
        accessibilityLabel={`Workspace: ${branch}`}
        onPress={() => setOpen(true)}
        // A taller target than the line itself.
        hitSlop={{ top: 4, bottom: 4 }}
        style={({ pressed }) => [styles.trigger, pressed && styles.pressed]}
      >
        <BranchLine
          current={value.mode === "current"}
          branch={branch}
          files={files}
          compact
        />
        <Icon name="chevronDown" size={12} color={colors.muted} />
      </Pressable>
      <WorkspaceSheet
        isPresented={open}
        onDismiss={dismiss}
        value={value}
        pending={loading ? pending : null}
        worktrees={worktrees}
        checkoutBranch={checkoutBranch}
        files={files}
        onOpenChanges={onOpenChanges}
        onChange={(next) => {
          if (keyOf(next) === keyOf(value)) return dismiss();
          onChange(next);
          setPending(keyOf(next));
        }}
      />
    </>
  );
}

/**
 * Native bottom sheet for where a new session works: the checkout and a new
 * worktree at the top level, and the existing worktrees a page further in,
 * pushed in the sheet's own navigation stack. A pick closes the sheet.
 */
function WorkspaceSheet({
  isPresented,
  onDismiss,
  value,
  pending,
  worktrees,
  checkoutBranch,
  files,
  onChange,
  onOpenChanges,
}: {
  isPresented: boolean;
  onDismiss: () => void;
  value: Workspace;
  /** The key of a pick still being read, spinning in its row. */
  pending: string | null;
  worktrees: Worktree[];
  checkoutBranch?: string;
  files: ChangedFile[];
  onChange: (value: Workspace) => void;
  onOpenChanges: () => void;
}) {
  const diff = useDiffTheme();
  const { additions, deletions } = totals(files);
  // Controlled so a reopened sheet starts at the top level, not the page it closed on.
  const [path, setPath] = useState<string[]>([]);
  // Present the route only after the native picker has finished dismissing.
  const afterDismiss = useRef<(() => void) | null>(null);
  const existing = value.mode === "existing" ? value.worktree : null;
  return (
    <WorkspaceBottomSheet
      isPresented={isPresented}
      onDismiss={onDismiss}
      onDidDismiss={() => {
        setPath([]);
        const openChanges = afterDismiss.current;
        afterDismiss.current = null;
        openChanges?.();
      }}
    >
      <NavigationStack path={path} onPathChange={setPath}>
        <List
          modifiers={[
            listStyle("insetGrouped"),
            listSectionSpacing(12),
            // The sheet's own glass shows through, as in the other sheets: the
            // list's background off, and the stack's behind each page as well,
            // which otherwise paints a pushed page opaque.
            scrollContentBackground("hidden"),
            containerBackground("clear", "navigation"),
            navigationTitle("Workspace"),
            navigationBarTitleDisplayMode("inline"),
          ]}
        >
          <Section title="Start the session in">
            <Choice
              symbol="folder"
              title="Current checkout"
              hint={checkoutBranch || "The project's own folder"}
              selected={value.mode === "current"}
              busy={pending === "current"}
              onPress={() => onChange({ mode: "current" })}
            />
            <Choice
              symbol="arrow.triangle.branch"
              title="New worktree"
              hint="A fresh branch, in a folder of its own"
              selected={value.mode === "worktree"}
              busy={pending === "worktree"}
              onPress={() => onChange({ mode: "worktree" })}
            />
            {worktrees.length > 0 ? (
              <NavigationLink value={EXISTING}>
                <HStack spacing={14}>
                  <RowIcon symbol="folder.badge.gearshape" />
                  <VStack alignment="leading" spacing={2}>
                    <Text>Existing worktree</Text>
                    <Text
                      modifiers={[
                        font({ textStyle: "footnote" }),
                        SECONDARY,
                        lineLimit(1),
                        truncationMode("middle"),
                      ]}
                    >
                      {existing
                        ? existing.name
                        : `${worktrees.length} ${worktrees.length === 1 ? "worktree" : "worktrees"}`}
                    </Text>
                  </VStack>
                  <Spacer />
                  {pending && existing?.path === pending ? (
                    <ProgressView />
                  ) : existing ? (
                    <Check />
                  ) : null}
                </HStack>
              </NavigationLink>
            ) : null}
          </Section>
          <Section
            footer={value.mode === "worktree" ? (
              <Text>Changes are available after the worktree is created.</Text>
            ) : undefined}
          >
            <Button
              modifiers={[
                buttonStyle("plain"),
                disabled(value.mode === "worktree" || !!pending),
              ]}
              onPress={() => {
                if (value.mode === "worktree" || pending) return;
                afterDismiss.current = onOpenChanges;
                onDismiss();
              }}
            >
              <HStack spacing={14} modifiers={[contentShape(shapes.rectangle())]}>
                <RowIcon symbol="plus.forwardslash.minus" />
                <VStack alignment="leading" spacing={2}>
                  <Text>View changes</Text>
                  <Text modifiers={[font({ textStyle: "footnote" }), SECONDARY]}>
                    {`${formatCount(files.length)} ${files.length === 1 ? "file" : "files"} changed`}
                  </Text>
                </VStack>
                <Spacer />
                <HStack
                  spacing={6}
                  modifiers={[font({ textStyle: "footnote" }), monospacedDigit()]}
                >
                  <Text modifiers={[foregroundStyle(diff.add.fg)]}>
                    {`+${formatCount(additions)}`}
                  </Text>
                  <Text modifiers={[foregroundStyle(diff.del.fg)]}>
                    {`-${formatCount(deletions)}`}
                  </Text>
                </HStack>
              </HStack>
            </Button>
          </Section>
        </List>
        <NavigationDestination value={EXISTING}>
          <List
            modifiers={[
              listStyle("insetGrouped"),
              scrollContentBackground("hidden"),
              containerBackground("clear", "navigation"),
              navigationTitle("Worktrees"),
              navigationBarTitleDisplayMode("inline"),
            ]}
          >
            <Section>
              {worktrees.map((tree) => (
                <Choice
                  key={tree.path}
                  symbol="arrow.triangle.branch"
                  title={tree.name}
                  hint={tree.path}
                  selected={existing?.path === tree.path}
                  busy={pending === tree.path}
                  onPress={() => onChange({ mode: "existing", worktree: tree })}
                />
              ))}
            </Section>
          </List>
        </NavigationDestination>
      </NavigationStack>
    </WorkspaceBottomSheet>
  );
}

/** The SwiftUI sheet exposes dismissal completion for opening the Changes modal. */
function WorkspaceBottomSheet({
  isPresented,
  onDismiss,
  onDidDismiss,
  children,
}: {
  isPresented: boolean;
  onDismiss: () => void;
  onDidDismiss: () => void;
  children: ReactNode;
}) {
  return (
    <Host style={{ position: "absolute" }} pointerEvents="none">
      <BottomSheet
        isPresented={isPresented}
        onIsPresentedChange={(presented) => {
          if (!presented) onDismiss();
        }}
        onDismiss={onDidDismiss}
      >
        <Group
          modifiers={[
            frame({ maxWidth: Infinity, alignment: "topLeading" }),
            presentationDetents([{ fraction: 0.49 }]),
            presentationDragIndicator("visible"),
          ]}
        >
          {children}
        </Group>
      </BottomSheet>
    </Host>
  );
}

/** A row to pick: its symbol, title and a line under it, checked when chosen
 *  and spinning while a pick of it is read. */
function Choice({
  symbol,
  title,
  hint,
  selected,
  busy,
  onPress,
}: {
  symbol: RowSymbol;
  title: string;
  hint: string;
  selected: boolean;
  /** Picked, and still being read. */
  busy: boolean;
  onPress: () => void;
}) {
  return (
    <Button onPress={onPress} modifiers={[buttonStyle("plain")]}>
      <HStack
        spacing={14}
        // The whole row answers a tap, not just its text.
        modifiers={[contentShape(shapes.rectangle())]}
      >
        <RowIcon symbol={symbol} />
        <VStack alignment="leading" spacing={2}>
          <Text modifiers={[lineLimit(1), truncationMode("middle")]}>{title}</Text>
          <Text
            modifiers={[
              font({ textStyle: "footnote" }),
              SECONDARY,
              lineLimit(1),
              truncationMode("middle"),
            ]}
          >
            {hint}
          </Text>
        </VStack>
        <Spacer />
        {busy ? <ProgressView /> : selected ? <Check /> : null}
      </HStack>
    </Button>
  );
}

function RowIcon({ symbol }: { symbol: RowSymbol }) {
  const colors = useColors();
  return (
    <Image
      systemName={symbol}
      size={18}
      color={colors.content}
      modifiers={[frame({ width: 24 })]}
    />
  );
}

function Check() {
  const colors = useColors();
  return <Image systemName="checkmark" size={16} color={colors.accent} />;
}

/**
 * The same line, for a session that has started: where it works, shown in its
 * composer, with nothing to pick.
 */
export function WorkspaceLine(props: {
  /** Working in the project's checkout rather than a worktree. */
  current: boolean;
  branch: string;
  files: ChangedFile[];
}) {
  return <BranchLine {...props} />;
}

/** The branch with its icon, then how many files changed and by how much;
 *  `compact` for the line under the header, the composer's size otherwise. */
function BranchLine({
  current,
  branch,
  files,
  compact = false,
}: {
  current: boolean;
  branch: string;
  files: ChangedFile[];
  compact?: boolean;
}) {
  const colors = useColors();
  const styles = useStyles();
  const diff = useDiffTheme();
  const { additions, deletions } = totals(files);
  const count = [styles.count, compact && styles.compact];
  return (
    <View style={styles.line}>
      <Icon name={current ? "folder" : "branch"} size={compact ? 13 : 14} color={colors.muted} />
      <RNText
        style={[styles.branch, compact && styles.compact]}
        numberOfLines={1}
        ellipsizeMode="middle"
      >
        {branch}
      </RNText>
      <RNText style={count} numberOfLines={1}>
        {`·  ${formatCount(files.length)} ${files.length === 1 ? "file" : "files"}`}
      </RNText>
      <RNText style={[count, { color: diff.add.fg }]}>{`+${formatCount(additions)}`}</RNText>
      <RNText style={[count, { color: diff.del.fg }]}>{`-${formatCount(deletions)}`}</RNText>
    </View>
  );
}

function totals(files: ChangedFile[]) {
  return {
    additions: files.reduce((sum, f) => sum + f.additions, 0),
    deletions: files.reduce((sum, f) => sum + f.deletions, 0),
  };
}

const useStyles = themedStyles((colors) => ({
  trigger: {
    flexShrink: 1,
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
    paddingVertical: 4,
    paddingHorizontal: 8,
    borderRadius: 8,
  },
  pressed: {
    opacity: 0.6,
  },
  line: {
    flexShrink: 1,
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
  },
  branch: {
    flexShrink: 1,
    color: colors.content,
    fontSize: 15,
    fontWeight: "500",
  },
  count: {
    color: colors.muted,
    fontSize: 15,
    fontVariant: ["tabular-nums"],
  },
  compact: {
    fontSize: 13,
  },
}));
