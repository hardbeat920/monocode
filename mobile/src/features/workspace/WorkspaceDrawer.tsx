import { Host, HStack, Label, Menu, Picker, Spacer } from "@expo/ui/swift-ui";
import {
  accessibilityLabel,
  contentShape,
  frame,
  pickerStyle,
  shapes,
  tag,
} from "@expo/ui/swift-ui/modifiers";
import { router, useGlobalSearchParams, usePathname } from "expo-router";
import { memo, useMemo, useState } from "react";
import { Alert, Image, Pressable, ScrollView, StyleSheet, Text, TextInput, View } from "react-native";
import Animated from "react-native-reanimated";
import { useSafeAreaInsets } from "react-native-safe-area-context";

import { useConnection, useMonoTools } from "@/features/connection/ConnectionProvider";
import { PixelMascot } from "@/features/mascots/PixelMascot";
import { ProjectMascot } from "@/features/mascots/ProjectMascot";
import { plainText } from "@/features/sessions/markdownParser";
import { HEADER_GAP, HEADER_HEIGHT, useKeyboardLift } from "@/features/sessions/SessionScreen";
import { MONOCODE_MARK } from "@/ui/brand";
import { CircleButton } from "@/ui/CircleButton";
import { useWarmDitherLoading } from "@/ui/DitherLoading";
import { GradualBlur } from "@/ui/GradualBlur";
import { Icon } from "@/ui/Icon";
import { LiquidView } from "@/ui/LiquidView";
import { isModalRoute } from "@/ui/SceneFade";
import { AnimationsPaused } from "@/ui/AnimationsPaused";
import { useDrawerActions, useDrawerOpen } from "@/ui/ScenePager";
import { Shimmer } from "@/ui/Shimmer";
import { radii, themedStyles, useColors, useScheme } from "@/ui/theme";

import { useUnreadMonos } from "./monoSeen";
import { pickProject } from "./projectPick";
import { projectForRoute } from "./routing";
import type { Mono, Project } from "./types";
import { useWorkspace } from "./WorkspaceProvider";

/** As the composer's pill, so the drawer's foot lines up with a chat's. */
const FOOTER_HEIGHT = 50;
/** How far past the header and footer the blur keeps fading. */
const FADE = 16;
/** The Monocode mark in the header. */
const MARK = 38;
/** A Mono row's mascot, as a chat list's avatar. */
const AVATAR = 40;
/** The rows' inset and their own padding, so a Mono's figure lines up with
 *  the header's buttons. */
const LIST_INSET = 6;
const ROW_INSET = 10;
const ROW_GAP = 14;

type Filter = "all" | "unread" | "working";
/** Which kinds the list shows. */
type Show = "both" | "monos" | "projects";

/**
 * The phone's counterpart of the desktop's rail, laid out as a messaging
 * app's chat list: the Monocode mark, the filter and a new Mono up top, level
 * with a screen's header; the Monos with their latest line and when; and at
 * Projects under their own label, and at the foot settings and search, level
 * with a screen's composer. Picking one switches the screen under it as the
 * drawer shuts.
 */
export function WorkspaceDrawer() {
  const drawer = useDrawerActions();
  const open = useDrawerOpen();
  // The switch's loading cover, drawn while the drawer is open to be picked from.
  useWarmDitherLoading(open);
  const insets = useSafeAreaInsets();
  const colors = useColors();
  const styles = useStyles();
  const { projects, monos: roster } = useWorkspace();
  const { command, refresh, selection, select, pairing } = useConnection();
  const tools = useMonoTools();
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  const [show, setShow] = useState<Show>("both");
  const [creating, setCreating] = useState(false);
  const unread = useUnreadMonos(pairing?.desktop.fingerprint, roster);
  const pathname = usePathname();
  const params = useGlobalSearchParams<{ projectId?: string; monoId?: string }>();
  const onMono = pathname.startsWith("/mono/");
  const onProject = pathname.startsWith("/project/");
  const activeMono = onMono ? params.monoId : undefined;
  const activeProject = onProject
    ? projectForRoute(projects, params.projectId)?.id
    : undefined;

  // The mono heard from last first, as a messaging app's chat list, narrowed
  // by the search and the filter.
  const needle = query.trim().toLowerCase();
  const monos = useMemo(
    () =>
      [...roster]
        .sort((a, b) => (b.lastActiveAt || 0) - (a.lastActiveAt || 0))
        .filter(
          (mono) =>
            (!needle || mono.name.toLowerCase().includes(needle)) &&
            (filter === "all" ||
              (filter === "unread" ? unread.has(mono.id) : mono.status !== "idle")),
        ),
    [roster, needle, filter, unread],
  );
  const shownProjects = projects.filter(
    (project) =>
      (!needle || project.name.toLowerCase().includes(needle)) &&
      // A project has no replies to read, so only "All" and "Working now" keep it.
      (filter === "all" || (filter === "working" && project.busy)),
  );

  // The drawer shuts at once over a loading cover, and the next screen is
  // built only once it is shut, so building it can't hitch the motion. Same
  // kind to same kind only swaps the params, so the frame stays mounted.
  const openProject = (id: string) => {
    if (id === activeProject) return drawer.close();
    drawer.closeThen(() => {
      // The project route takes a pick in place; any other screen is replaced.
      if (!pickProject(id))
        router.replace({ pathname: "/project/[projectId]", params: { projectId: id } });
    });
  };
  const openMono = (id: string) => {
    if (id === activeMono) return drawer.close();
    // Mono to Mono only swaps the conversation in the same screen, which is
    // light enough to switch as the drawer shuts.
    if (onMono) {
      drawer.close();
      return router.setParams({ monoId: id });
    }
    drawer.closeThen(() =>
      router.replace({ pathname: "/mono/[monoId]", params: { monoId: id } }),
    );
  };
  // Settings rises over the open drawer, which is still there once it's shut.
  const openSettings = () => {
    if (!isModalRoute(pathname)) router.push("/settings");
  };
  // A new Mono on the desktop, shown on this phone at once, then its chat with
  // its look open to name it.
  const createMono = async () => {
    if (creating) return;
    setCreating(true);
    try {
      const { monoId } = await command<{ monoId: string }>("mono.create");
      if (selection) await select({ ...selection, monoIds: [...selection.monoIds, monoId] });
      await refresh();
      drawer.closeThen(() => {
        router.replace({ pathname: "/mono/[monoId]", params: { monoId } });
        router.push({ pathname: "/mono-profile/[monoId]/edit", params: { monoId } });
      });
    } catch (reason) {
      Alert.alert("Couldn’t create a Mono", reason instanceof Error ? reason.message : String(reason));
    } finally {
      setCreating(false);
    }
  };

  // Each kind under its label, as the filter lets it show.
  const listedMonos = show === "projects" ? [] : monos;
  const listedProjects = show === "monos" ? [] : shownProjects;
  const headerTop = insets.top + HEADER_GAP;
  const headerBottom = headerTop + HEADER_HEIGHT + 12;
  const footerBottom = Math.max(insets.bottom, 12);
  const footerTop = footerBottom + FOOTER_HEIGHT + 12;
  // The search rides the keyboard, resting just above it.
  const lift = useKeyboardLift(footerBottom - 10);
  const nothing = !listedMonos.length && !listedProjects.length;

  return (
    // A shut drawer's mascots hold still; they move again as it opens.
    <AnimationsPaused value={!open}>
      <View style={styles.container}>
        <ScrollView
          style={StyleSheet.absoluteFill}
          contentContainerStyle={{ paddingTop: headerBottom, paddingBottom: footerTop + FADE }}
          scrollIndicatorInsets={{ top: headerBottom, bottom: footerTop }}
          showsVerticalScrollIndicator={false}
          keyboardShouldPersistTaps="handled"
          keyboardDismissMode="on-drag"
        >
          {listedMonos.length ? (
            <View style={styles.list}>
              <Text style={styles.sectionLabel}>Monos</Text>
              {listedMonos.map((mono) => (
                <MonoRow
                  key={mono.id}
                  mono={mono}
                  unread={unread.has(mono.id)}
                  active={mono.id === activeMono}
                  onPress={openMono}
                />
              ))}
            </View>
          ) : null}
          {listedProjects.length ? (
            <View style={[styles.list, listedMonos.length > 0 && styles.section]}>
              <Text style={styles.sectionLabel}>Projects</Text>
              {listedProjects.map((project) => (
                <ProjectRow
                  key={project.id}
                  project={project}
                  active={project.id === activeProject}
                  onPress={openProject}
                />
              ))}
            </View>
          ) : null}
          {nothing ? (
            <Text style={styles.empty}>
              {needle || filter !== "all" || show !== "both"
                ? "Nothing matches."
                : "No Monos or projects on this phone yet."}
            </Text>
          ) : null}
        </ScrollView>

        <GradualBlur edge="top" height={headerBottom + FADE} color={colors.drawer} />
        <GradualBlur edge="bottom" height={footerTop + FADE} color={colors.drawer} />

        <View style={[styles.header, { top: headerTop }]}>
          <Image
            source={MONOCODE_MARK}
            style={[styles.mark, { tintColor: colors.content }]}
            accessibilityIgnoresInvertColors
          />
          <View style={styles.headerActions}>
            <FilterMenu filter={filter} onFilter={setFilter} show={show} onShow={setShow} />
            {/* The older desktop app can't make a Mono from the phone. */}
            {tools ? <CircleButton icon="add" label="New Mono" onPress={() => void createMono()} /> : null}
          </View>
        </View>

        <Animated.View style={[styles.foot, { bottom: footerBottom }, lift]}>
          <View style={styles.footer}>
          <CircleButton icon="settings" label="Settings" onPress={openSettings} size={FOOTER_HEIGHT} />
          <LiquidView borderRadius={FOOTER_HEIGHT / 2} style={styles.searchFrame} innerStyle={styles.search}>
            <Icon name="search" size={18} color={colors.muted} />
            <TextInput
              value={query}
              onChangeText={setQuery}
              placeholder="Search"
              placeholderTextColor={colors.muted}
              keyboardAppearance={useScheme()}
              returnKeyType="search"
              autoCorrect={false}
              clearButtonMode="while-editing"
              style={styles.searchInput}
            />
          </LiquidView>
          </View>
        </Animated.View>
      </View>
    </AnimationsPaused>
  );
}

const FILTERS: { value: Filter; label: string; symbol: "tray.full" | "circlebadge" | "bolt" }[] = [
  { value: "all", label: "All", symbol: "tray.full" },
  { value: "unread", label: "Unread", symbol: "circlebadge" },
  { value: "working", label: "Working now", symbol: "bolt" },
];

const SHOWS: { value: Show; label: string; symbol: "square.stack" | "person.2" | "folder" }[] = [
  { value: "both", label: "Monos and projects", symbol: "square.stack" },
  { value: "monos", label: "Monos", symbol: "person.2" },
  { value: "projects", label: "Projects", symbol: "folder" },
];

/** The round button beside a new Mono: a native menu of which kinds to show
 *  and which of them. Lit while either narrows the list. */
function FilterMenu({
  filter,
  onFilter,
  show,
  onShow,
}: {
  filter: Filter;
  onFilter: (value: Filter) => void;
  show: Show;
  onShow: (value: Show) => void;
}) {
  const colors = useColors();
  const on = filter !== "all" || show !== "both";
  return (
    <View>
      <LiquidView borderRadius={HEADER_HEIGHT / 2} innerStyle={filterStyles.round}>
        <Icon name="filter" size={20} color={on ? colors.accent : colors.content} />
      </LiquidView>
      {/* The native menu lies over the glass, its label an invisible box of
          the button's size, as in SessionFilterMenu. */}
      <Host style={StyleSheet.absoluteFill}>
        <Menu
          modifiers={[accessibilityLabel(on ? "Filter, on" : "Filter")]}
          label={
            <HStack modifiers={[frame({ width: HEADER_HEIGHT, height: HEADER_HEIGHT }), contentShape(shapes.circle())]}>
              <Spacer />
            </HStack>
          }
        >
          <Picker
            selection={show}
            onSelectionChange={(next) => onShow(next as Show)}
            modifiers={[pickerStyle("inline")]}
          >
            {SHOWS.map((option) => (
              <Label key={option.value} title={option.label} systemImage={option.symbol} modifiers={[tag(option.value)]} />
            ))}
          </Picker>
          <Picker
            selection={filter}
            onSelectionChange={(next) => onFilter(next as Filter)}
            modifiers={[pickerStyle("inline")]}
          >
            {FILTERS.map((option) => (
              <Label key={option.value} title={option.label} systemImage={option.symbol} modifiers={[tag(option.value)]} />
            ))}
          </Picker>
        </Menu>
      </Host>
    </View>
  );
}

const filterStyles = StyleSheet.create({
  round: {
    width: HEADER_HEIGHT,
    height: HEADER_HEIGHT,
  },
});

/** A Mono as a chat in a messaging app's list: its mascot, its name and when
 *  it was last active, and under them what it is doing or its last line. */
const MonoRow = memo(function MonoRow({
  mono,
  unread,
  active,
  onPress,
}: {
  mono: Mono;
  /** A reply it sent that the user hasn't seen. */
  unread: boolean;
  active: boolean;
  onPress: (id: string) => void;
}) {
  const styles = useStyles();
  // The shimmer only runs where it can be seen.
  const shown = useDrawerOpen();
  const detail =
    mono.status === "working"
      ? "Working…"
      : mono.status === "needs-you"
        ? "Needs you"
        : oneLine(mono.lastMessage) || "No messages yet";
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={`${mono.name}, ${detail}`}
      accessibilityState={{ selected: active }}
      onPress={() => onPress(mono.id)}
      style={({ pressed }) => [styles.row, (active || pressed) && styles.rowActive]}
    >
      <PixelMascot name={mono.mascot} color={mono.color} status={mono.status} size={AVATAR} centered />
      <View style={styles.rowText}>
        <View style={styles.rowTop}>
          <Text style={styles.rowLabel} numberOfLines={1}>
            {mono.name}
          </Text>
          {mono.lastActiveAt ? (
            <Text style={[styles.rowTime, unread && styles.rowTimeUnread]}>{when(mono.lastActiveAt)}</Text>
          ) : null}
        </View>
        <View style={styles.rowTop}>
          {mono.status === "working" && shown ? (
            <Shimmer style={[styles.rowDetail, styles.rowDetailFill]}>{detail}</Shimmer>
          ) : (
            <Text
              style={[styles.rowDetail, styles.rowDetailFill, mono.status === "needs-you" && styles.rowNeedsYou]}
              numberOfLines={1}
            >
              {detail}
            </Text>
          )}
          {unread ? <View style={styles.unread} /> : null}
        </View>
      </View>
    </Pressable>
  );
});

/** When a Mono was last active, as a chat list dates its rows: the time
 *  today, Yesterday, else the day. */
function when(at: number) {
  const date = new Date(at);
  const today = new Date();
  if (date.toDateString() === today.toDateString())
    return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  const yesterday = new Date(today);
  yesterday.setDate(today.getDate() - 1);
  if (date.toDateString() === yesterday.toDateString()) return "Yesterday";
  return date.toLocaleDateString([], { month: "short", day: "numeric" });
}

/** The start of a message as one plain line: its markdown read for its
 *  words, so **bold** shows as bold rather than its asterisks. Only as much
 *  as one line can show is read. */
function oneLine(text: string) {
  // A reply that is only a reaction, as the chat shows it, by its emoji.
  const reaction = text.trim().match(/^<reaction>\s*(.{1,16}?)\s*<\/reaction>$/su);
  if (reaction) return `Reacted ${reaction[1]}`;
  return plainText(text.slice(0, 600));
}

/** A project as a row of the list, under the Monos: its mascot and name. */
const ProjectRow = memo(function ProjectRow({
  project,
  active,
  onPress,
}: {
  project: Project;
  active: boolean;
  onPress: (id: string) => void;
}) {
  const styles = useStyles();
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={project.name}
      accessibilityState={{ selected: active }}
      onPress={() => onPress(project.id)}
      style={({ pressed }) => [styles.projectRow, (active || pressed) && styles.rowActive]}
    >
      <View style={styles.projectIcon}>
        <ProjectMascot
          project={project.path}
          name={project.mascot}
          color={project.color}
          size={24}
          active={project.busy}
        />
      </View>
      <Text style={[styles.rowLabel, styles.rowText]} numberOfLines={1}>
        {project.name}
      </Text>
    </Pressable>
  );
});

const useStyles = themedStyles((colors) => ({
  container: {
    flex: 1,
    backgroundColor: colors.drawer,
  },
  header: {
    position: "absolute",
    left: 0,
    right: 0,
    height: HEADER_HEIGHT,
    flexDirection: "row",
    alignItems: "center",
    justifyContent: "space-between",
    paddingHorizontal: 16,
  },
  headerActions: {
    flexDirection: "row",
    alignItems: "center",
    gap: 10,
  },
  // The Monocode mark, centred on a row's figure below it.
  mark: {
    width: MARK,
    height: MARK,
    marginLeft: LIST_INSET + ROW_INSET + (AVATAR - MARK) / 2 - 16,
  },
  foot: {
    position: "absolute",
    left: 0,
    right: 0,
  },
  section: {
    marginTop: 18,
  },
  // Dimmed heading over each kind, inset like the rows' text.
  sectionLabel: {
    paddingHorizontal: ROW_INSET,
    marginBottom: 4,
    color: colors.muted,
    fontSize: 13,
    fontWeight: "500",
  },
  pressed: {
    opacity: 0.6,
  },
  list: {
    paddingHorizontal: LIST_INSET,
    gap: 2,
  },
  row: {
    flexDirection: "row",
    alignItems: "center",
    gap: ROW_GAP,
    minHeight: 64,
    paddingLeft: ROW_INSET,
    // Clear of the scene's edge, which the drawer meets on the right.
    paddingRight: ROW_INSET + 12,
    paddingVertical: 10,
    borderRadius: radii.card - 6,
    borderCurve: "continuous",
  },
  projectRow: {
    flexDirection: "row",
    alignItems: "center",
    gap: ROW_GAP,
    minHeight: 52,
    paddingHorizontal: ROW_INSET,
    paddingVertical: 8,
    borderRadius: radii.row,
    borderCurve: "continuous",
  },
  // The width of a Mono's figure, so the logo above centres on it either way.
  projectIcon: {
    width: AVATAR,
    alignItems: "center",
  },
  rowActive: {
    backgroundColor: colors.selection,
  },
  rowText: {
    flex: 1,
    minWidth: 0,
    gap: 3,
  },
  rowTop: {
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
  },
  rowLabel: {
    flex: 1,
    color: colors.content,
    fontSize: 17,
    fontWeight: "600",
  },
  rowTime: {
    color: colors.muted,
    fontSize: 14,
    fontVariant: ["tabular-nums"],
  },
  rowTimeUnread: {
    color: colors.accent,
  },
  rowDetail: {
    color: colors.muted,
    fontSize: 15,
  },
  rowDetailFill: {
    flex: 1,
  },
  rowNeedsYou: {
    color: colors.caution,
  },
  unread: {
    width: 10,
    height: 10,
    borderRadius: 5,
    backgroundColor: colors.accent,
  },
  empty: {
    paddingHorizontal: LIST_INSET + ROW_INSET,
    paddingTop: 12,
    color: colors.muted,
    fontSize: 15,
  },
  footer: {
    flexDirection: "row",
    alignItems: "center",
    gap: 10,
    paddingHorizontal: 16,
  },
  searchFrame: {
    flex: 1,
  },
  search: {
    height: FOOTER_HEIGHT,
    flexDirection: "row",
    alignItems: "center",
    gap: 10,
    paddingHorizontal: 16,
  },
  searchInput: {
    flex: 1,
    height: FOOTER_HEIGHT,
    color: colors.content,
    fontSize: 17,
  },
}));
