import {
  Button,
  HStack,
  Host,
  Image,
  LazyVStack,
  ProgressView,
  Rectangle,
  RNHostView,
  ScrollView,
  Spacer,
  Text,
  VStack,
} from "@expo/ui/swift-ui";
import {
  animation,
  Animation,
  background,
  buttonStyle,
  clipShape,
  contentShape,
  controlSize,
  font,
  foregroundStyle,
  frame,
  lineLimit,
  opacity,
  padding,
  refreshable,
  rotationEffect,
  shapes,
} from "@expo/ui/swift-ui/modifiers";
import { router } from "expo-router";
import { Fragment, useState, type ReactNode } from "react";
import { StyleSheet, View } from "react-native";

import { useFileSearch, type FileMatch } from "@/features/connection/useFileSearch";
import { useFileTree, type TreeRow } from "@/features/connection/useFileTree";
import { useSessionLayout } from "@/features/sessions/SessionScreen";
import { GradualBlur } from "@/ui/GradualBlur";
import { useColors } from "@/ui/theme";

import { FileSearchBar, SEARCH_BAR } from "./FileSearchBar";
import { FileTypeIcon } from "./FileTypeIcon";

const ICON = 18;
/** How far each level of the tree steps in. */
const INDENT = 16;
/** The Changes cards' look, flattened onto the page: margin, radius, fill,
 *  dividers, inner inset and the gap after a row's icon. */
const CARD_MARGIN = 10;
const CARD_RADIUS = 26;
const INSET = 16;
const ICON_GAP = 10;
/** Where the Changes page's first card starts below its top inset, matched here. */
const CARD_OFFSET = 4;

/**
 * The working copy's files as a tree, after the desktop's file explorer, in
 * one native card. Folders open in place: the rows they add, the card's height
 * and the chevron move together in one animation, so nothing shows through
 * while it grows. Folders come first and ignored entries are dimmed. Tapping a
 * file opens it in a sheet; pulling down reads the open folders again. The
 * search field where the composer rests finds files anywhere in the working
 * copy, ranked on the desktop, and lists them in the card instead. Read only,
 * like the Changes page.
 */
export function FilesView({
  projectId,
  worktreePath,
  active,
}: {
  projectId: string;
  worktreePath?: string;
  /** Whether the page is showing, so folders are only read when looked at. */
  active: boolean;
}) {
  const colors = useColors();
  const { pageTop: top, dockBottom } = useSessionLayout();
  // Room for the search bar where the composer rests, and the blur under it.
  const bottom = dockBottom + SEARCH_BAR + 16;
  const tree = useFileTree(projectId, active, worktreePath);
  const [query, setQuery] = useState("");
  const search = useFileSearch(projectId, query, worktreePath);
  const searching = query.trim().length > 0;

  const openFile = (path: string) =>
    router.push({
      pathname: "/file",
      params: { projectId, path, ...(worktreePath ? { worktreePath } : {}) },
    });
  const press = (row: TreeRow) => {
    if (row.entry.isDir) void tree.toggle(row.path);
    else openFile(row.path);
  };

  // A line in the card when there are no rows to show.
  const note = (text: string) => (
    <Text
      modifiers={[
        font({ size: 15 }),
        foregroundStyle(colors.muted),
        padding({ vertical: 14, horizontal: INSET }),
        frame({ maxWidth: Infinity, alignment: "leading" }),
      ]}
    >
      {text}
    </Text>
  );

  let rows: ReactNode;
  if (searching)
    rows =
      search.results.length > 0
        ? search.results.map((match, index) => (
            <Fragment key={match.path}>
              {index > 0 ? <Divider depth={0} /> : null}
              <MatchRow match={match} onPress={() => openFile(match.path)} />
            </Fragment>
          ))
        : note(search.error ?? (search.searching ? "Searching…" : "No matching files."));
  else
    rows =
      tree.rows.length > 0
        ? tree.rows.map((row, index) => (
            <Fragment key={row.path}>
              {index > 0 ? <Divider depth={row.depth} /> : null}
              <Row row={row} onPress={() => press(row)} />
            </Fragment>
          ))
        : note(tree.error ?? (tree.ready ? "This folder is empty." : "Reading from desktop…"));

  return (
    <>
      {/* Ignores the screen's safe areas but not the keyboard's, so while the
          search has the keyboard up the list can still scroll to its end. */}
      <Host ignoreSafeArea="container" style={styles.host}>
        <ScrollView showsIndicators={false} modifiers={[refreshable(() => tree.refresh())]}>
          <VStack
            modifiers={[
              padding({
                top: top + CARD_OFFSET,
                bottom,
                leading: CARD_MARGIN,
                trailing: CARD_MARGIN,
              }),
            ]}
          >
            <LazyVStack
              alignment="leading"
              spacing={0}
              modifiers={[
                background(colors.listCard, shapes.roundedRectangle({ cornerRadius: CARD_RADIUS })),
                clipShape("roundedRectangle", CARD_RADIUS),
                // Rows coming and going, the card's height and the chevrons
                // all animate together whenever a folder opens or closes.
                animation(
                  Animation.smooth({ duration: 0.3 }),
                  searching ? -1 - search.results.length : tree.rows.length,
                ),
              ]}
            >
              {rows}
            </LazyVStack>
          </VStack>
        </ScrollView>
      </Host>
      <GradualBlur edge="bottom" height={bottom} color={colors.background} />
      <FileSearchBar value={query} onChange={setQuery} />
    </>
  );
}

function Row({ row, onPress }: { row: TreeRow; onPress: () => void }) {
  const colors = useColors();
  const { entry } = row;
  return (
    <Button onPress={onPress} modifiers={[buttonStyle("plain")]}>
      <HStack
        spacing={ICON_GAP}
        modifiers={[
          padding({ vertical: 13, leading: INSET + row.depth * INDENT, trailing: INSET }),
          frame({ maxWidth: Infinity }),
          // The whole row answers a tap, not just its icon and name.
          contentShape(shapes.rectangle()),
          ...(entry.ignored ? [opacity(0.45)] : []),
        ]}
      >
        {/* A folder waiting on its entries shows a spinner where its icon goes. */}
        {row.loading ? (
          <ProgressView modifiers={[controlSize("small"), frame({ width: ICON, height: ICON })]} />
        ) : (
          <RNHostView matchContents>
            <View style={styles.icon}>
              {/* Always the closed folder: swapping icons as it opens flashes, as
                  the hosted icon redraws a frame behind the animation. */}
              <FileTypeIcon name={entry.name} isDir={entry.isDir} size={ICON} />
            </View>
          </RNHostView>
        )}
        <Text modifiers={[font({ size: 16 }), lineLimit(1), foregroundStyle(colors.content)]}>
          {entry.name}
        </Text>
        <Spacer />
        {row.error ? (
          <Text modifiers={[font({ size: 13 }), foregroundStyle(colors.muted), lineLimit(1)]}>
            {row.error}
          </Text>
        ) : null}
        {entry.isDir ? (
          <Image
            systemName="chevron.right"
            size={13}
            color={colors.muted}
            modifiers={[rotationEffect(row.expanded ? 90 : 0)]}
          />
        ) : null}
      </HStack>
    </Button>
  );
}

/** A search result: its icon and name, with the folder it is in beside it. */
function MatchRow({ match, onPress }: { match: FileMatch; onPress: () => void }) {
  const colors = useColors();
  const dir = match.path.slice(0, Math.max(0, match.path.length - match.name.length - 1));
  return (
    <Button onPress={onPress} modifiers={[buttonStyle("plain")]}>
      <HStack
        spacing={ICON_GAP}
        modifiers={[
          padding({ vertical: 13, horizontal: INSET }),
          frame({ maxWidth: Infinity }),
          contentShape(shapes.rectangle()),
        ]}
      >
        <RNHostView matchContents>
          <View style={styles.icon}>
            <FileTypeIcon name={match.name} isDir={false} size={ICON} />
          </View>
        </RNHostView>
        <Text modifiers={[font({ size: 16 }), lineLimit(1), foregroundStyle(colors.content)]}>
          {match.name}
        </Text>
        {dir ? (
          <Text modifiers={[font({ size: 13 }), foregroundStyle(colors.muted), lineLimit(1)]}>
            {dir}
          </Text>
        ) : null}
        <Spacer />
      </HStack>
    </Button>
  );
}

/** A hairline between rows, inset to where the row's name starts. */
function Divider({ depth }: { depth: number }) {
  const colors = useColors();
  return (
    <Rectangle
      modifiers={[
        frame({ height: StyleSheet.hairlineWidth }),
        foregroundStyle(colors.listDivider),
        padding({ leading: INSET + depth * INDENT + ICON + ICON_GAP }),
      ]}
    />
  );
}

const styles = StyleSheet.create({
  host: {
    flex: 1,
  },
  icon: {
    width: ICON,
    height: ICON,
  },
});
