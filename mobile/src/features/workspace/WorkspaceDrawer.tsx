import { Label, Picker } from "@expo/ui/swift-ui";
import { pickerStyle, tag } from "@expo/ui/swift-ui/modifiers";
import { router, useGlobalSearchParams, usePathname } from "expo-router";
import { memo, useState } from "react";
import { Image, Pressable, ScrollView, StyleSheet, Text, TextInput, View } from "react-native";
import Animated from "react-native-reanimated";
import { useSafeAreaInsets } from "react-native-safe-area-context";

import { ProjectMascot } from "@/features/mascots/ProjectMascot";
import { HEADER_GAP, HEADER_HEIGHT, useKeyboardLift } from "@/features/sessions/SessionScreen";
import { MONOCODE_MARK } from "@/ui/brand";
import { CircleButton } from "@/ui/CircleButton";
import { useWarmDitherLoading } from "@/ui/DitherLoading";
import { GradualBlur } from "@/ui/GradualBlur";
import { Icon } from "@/ui/Icon";
import { GlassMenuButton } from "@/ui/GlassMenuButton";
import { LiquidView } from "@/ui/LiquidView";
import { isModalRoute } from "@/ui/SceneFade";
import { AnimationsPaused } from "@/ui/AnimationsPaused";
import { useDrawerActions, useDrawerOpen } from "@/ui/ScenePager";
import { radii, themedStyles, useColors, useScheme } from "@/ui/theme";

import { pickProject } from "./projectPick";
import { projectForRoute } from "./routing";
import type { Project } from "./types";
import { useWorkspace } from "./WorkspaceProvider";

/** As the composer's pill, so the drawer's foot lines up with a chat's. */
const FOOTER_HEIGHT = 50;
/** How far past the header and footer the blur keeps fading. */
const FADE = 16;
/** The Monocode mark in the header. */
const MARK = 38;
/** A project row's mascot column. */
const AVATAR = 40;
/** The rows' inset and their own padding, so a project's figure lines up with
 *  the header's buttons. */
const LIST_INSET = 6;
const ROW_INSET = 10;
const ROW_GAP = 14;

type Filter = "all" | "working";

/**
 * The phone's counterpart of the desktop's rail: the Monocode mark and the
 * filter up top, level with a screen's header; the projects under their own
 * label; and at the foot settings and search, level with a screen's composer.
 * Picking a project switches the screen under it as the drawer shuts. Monos
 * are not here: each sits pinned on its projects' Sessions pages.
 */
export function WorkspaceDrawer() {
  const drawer = useDrawerActions();
  const open = useDrawerOpen();
  // The switch's loading cover, drawn while the drawer is open to be picked from.
  useWarmDitherLoading(open);
  const insets = useSafeAreaInsets();
  const colors = useColors();
  const styles = useStyles();
  const { projects } = useWorkspace();
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  const pathname = usePathname();
  const params = useGlobalSearchParams<{ projectId?: string }>();
  const onProject = pathname.startsWith("/project/");
  const activeProject = onProject
    ? projectForRoute(projects, params.projectId)?.id
    : undefined;

  // Narrowed by the search and the filter.
  const needle = query.trim().toLowerCase();
  const shownProjects = projects.filter(
    (project) =>
      (!needle || project.name.toLowerCase().includes(needle)) &&
      (filter === "all" || project.busy),
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
  // Settings rises over the open drawer, which is still there once it's shut.
  const openSettings = () => {
    if (!isModalRoute(pathname)) router.push("/settings");
  };
  const headerTop = insets.top + HEADER_GAP;
  const headerBottom = headerTop + HEADER_HEIGHT + 12;
  const footerBottom = Math.max(insets.bottom, 12);
  const footerTop = footerBottom + FOOTER_HEIGHT + 12;
  // The search rides the keyboard, resting just above it.
  const lift = useKeyboardLift(footerBottom - 10);
  const nothing = !shownProjects.length;

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
          {shownProjects.length ? (
            <View style={styles.list}>
              <Text style={styles.sectionLabel}>Projects</Text>
              {shownProjects.map((project) => (
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
              {needle || filter !== "all"
                ? "Nothing matches."
                : "No projects on this phone yet."}
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
            <FilterMenu filter={filter} onFilter={setFilter} />
          </View>
        </View>

        <Animated.View style={[styles.foot, { bottom: footerBottom }, lift]}>
          <View style={styles.footer}>
          <CircleButton icon="settings" label="Settings" onPress={openSettings} size={FOOTER_HEIGHT} />
          <LiquidView interactive borderRadius={FOOTER_HEIGHT / 2} style={styles.searchFrame} innerStyle={styles.search}>
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

const FILTERS: { value: Filter; label: string; symbol: "tray.full" | "bolt" }[] = [
  { value: "all", label: "All", symbol: "tray.full" },
  { value: "working", label: "Working now", symbol: "bolt" },
];

/** The round button in the header: a native menu of which projects to show.
 *  Lit while it narrows the list. */
function FilterMenu({
  filter,
  onFilter,
}: {
  filter: Filter;
  onFilter: (value: Filter) => void;
}) {
  return (
    <GlassMenuButton
      icon="filter"
      symbol="slider.horizontal.3"
      label="Filter"
      size={HEADER_HEIGHT}
      on={filter !== "all"}
    >
      <Picker
        selection={filter}
        onSelectionChange={(next) => onFilter(next as Filter)}
        modifiers={[pickerStyle("inline")]}
      >
        {FILTERS.map((option) => (
          <Label key={option.value} title={option.label} systemImage={option.symbol} modifiers={[tag(option.value)]} />
        ))}
      </Picker>
    </GlassMenuButton>
  );
}

/** A project as a row of the list: its mascot and name. */
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
  // Dimmed heading over the projects, inset like the rows' text.
  sectionLabel: {
    paddingHorizontal: ROW_INSET,
    marginBottom: 4,
    color: colors.muted,
    fontSize: 13,
    fontWeight: "500",
  },
  list: {
    paddingHorizontal: LIST_INSET,
    gap: 2,
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
  rowLabel: {
    flex: 1,
    color: colors.content,
    fontSize: 17,
    fontWeight: "600",
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
