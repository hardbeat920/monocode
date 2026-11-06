import { BottomSheet, RNHostView } from "@expo/ui";
import * as Haptics from "expo-haptics";
import { useState, type ReactNode } from "react";
import {
  Pressable,
  ScrollView,
  Text,
  useWindowDimensions,
  View,
} from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";

import { ProjectMascot } from "@/features/mascots/ProjectMascot";
import { projectColor } from "@/features/workspace/projectColors";
import { Icon, type IconName } from "@/ui/Icon";
import { useSheetColor } from "@/ui/sheetColor";
import {
  SHEET_INSET,
  SHEET_ROW,
  SHEET_TOP,
  SheetCard,
  SheetDivider,
  SheetHeading,
  SheetTitle,
} from "@/ui/SheetParts";
import { SHEET_FADE, SHEET_FILL, SheetBody, SheetFade } from "@/ui/SheetScroll";
import { useTextScale } from "@/ui/textScale";
import { themedStyles, useColors, withAlpha, type Palette } from "@/ui/theme";

import { Avatar } from "./Avatar";
import { itemState } from "./ItemMark";
import { ME, type InboxFacets, type InboxUpdated } from "./model";
import { SOURCE_NAMES, SourceLogo } from "./ProviderMark";
import type {
  InboxClient,
  InboxItem,
  InboxOptions,
  InboxSource,
  InboxStatus,
} from "./protocol";

export type FilterKind = "place" | "author" | "status" | "kind" | "updated" | "source";

const STATUSES: { id: InboxStatus; label: string }[] = [
  { id: "open", label: "Open" },
  { id: "draft", label: "Draft" },
  { id: "closed", label: "Closed" },
  { id: "merged", label: "Merged" },
];
const KINDS: { id: InboxItem["kind"]; label: string }[] = [
  { id: "issue", label: "Issues" },
  { id: "pr", label: "Pull requests" },
];
const TIMES: { id: InboxUpdated; label: string }[] = [
  { id: "today", label: "Today" },
  { id: "7d", label: "Last 7 days" },
  { id: "30d", label: "Last 30 days" },
];

const TITLES: Record<FilterKind, string> = {
  place: "Projects",
  author: "Author",
  status: "Status",
  kind: "Type",
  updated: "Updated",
  source: "Source",
};

/** The sheet's parts, for working out how tall it needs to be. */
const TITLE = 22 + SHEET_FADE;
const HEADING = 40;
/** Under the last card. */
const FOOT = 12;
/** The room a row's leading mark takes, so labels line up down a card. */
const LEADING = 28;
/** A row's line of text at the default size, and the space above and below it. */
const ROW_LINE = 20;
const ROW_PADDING = 8;
/** Where a sheet stops at first; a longer list can be pulled up to full height. */
const SHEET_MOST = 0.62;
/** How far a chip's text grows: past this, too few chips fit to pick between. */
const CHIP_TEXT_MAX = 1.6;
/** A chip's longest label at the default size. */
const CHIP_LABEL_WIDTH = 160;

const toggled = <T,>(list: readonly T[] | undefined, value: T) =>
  list?.includes(value) ? list.filter((entry) => entry !== value) : [...(list ?? []), value];

/** A place's name: the phone's for a project, else what the desktop calls it. */
export function placeName(id: string, options: InboxOptions, names: ReadonlyMap<string, string>) {
  return (
    names.get(id) ??
    options.places.find((place) => place.id === id)?.name ??
    id.replace(/^(repo|linear):/, "")
  );
}

/** Whether a place is a project, which has a mascot, rather than a tracker's. */
function isProject(id: string, options: InboxOptions, names: ReadonlyMap<string, string>) {
  return (
    names.has(id) || options.places.find((place) => place.id === id)?.kind === "project"
  );
}

/** What a filter's chip says: its name until it is set, then what it is set to. */
export function filterLabel(
  kind: FilterKind,
  facets: InboxFacets,
  place: (id: string) => string,
) {
  const many = (list: readonly string[] | undefined, one: (id: string) => string, plural: string) =>
    !list?.length ? TITLES[kind] : list.length === 1 ? one(list[0]) : `${list.length} ${plural}`;
  switch (kind) {
    case "place":
      return many(facets.places, place, "projects");
    case "author":
      return many(facets.authors, (id) => (id === ME ? "Me" : id), "authors");
    case "status":
      return many(
        facets.statuses,
        (id) => STATUSES.find((status) => status.id === id)?.label ?? id,
        "statuses",
      );
    case "source":
      return many(facets.sources, (id) => SOURCE_NAMES[id as InboxSource] ?? id, "sources");
    case "kind":
      return KINDS.find((entry) => entry.id === facets.kind)?.label ?? TITLES.kind;
    case "updated":
      return TIMES.find((entry) => entry.id === facets.updated)?.label ?? TITLES.updated;
  }
}

function isSet(kind: FilterKind, facets: InboxFacets) {
  switch (kind) {
    case "place":
      return !!facets.places?.length;
    case "author":
      return !!facets.authors?.length;
    case "status":
      return !!facets.statuses?.length;
    case "source":
      return !!facets.sources?.length;
    case "kind":
      return !!facets.kind;
    case "updated":
      return !!facets.updated;
  }
}

export function Chip({
  label,
  selected,
  onPress,
  leading,
  trailing,
  count,
  hint,
}: {
  label: string;
  selected: boolean;
  onPress: () => void;
  leading?: ReactNode;
  trailing?: ReactNode;
  count?: number;
  hint?: string;
}) {
  const styles = useStyles();
  const textScale = Math.min(useTextScale(), CHIP_TEXT_MAX);
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={label}
      accessibilityHint={hint}
      accessibilityState={{ selected }}
      onPress={() => {
        void Haptics.selectionAsync();
        onPress();
      }}
      style={[styles.chip, selected && styles.chipOn]}
    >
      {leading}
      <Text
        numberOfLines={1}
        maxFontSizeMultiplier={CHIP_TEXT_MAX}
        style={[
          styles.chipLabel,
          { maxWidth: CHIP_LABEL_WIDTH * textScale },
          selected && styles.chipLabelOn,
        ]}
      >
        {label}
      </Text>
      {count ? (
        <Text
          maxFontSizeMultiplier={CHIP_TEXT_MAX}
          style={[styles.chipCount, selected && styles.chipLabelOn]}
        >
          {count}
        </Text>
      ) : null}
      {trailing}
    </Pressable>
  );
}

/** The chips that open a filter's sheet, with Clear ahead of them once one is set. */
export function FilterChips({
  facets,
  onChange,
  options,
  canAssign,
  names,
  client,
  onOpen,
}: {
  facets: InboxFacets;
  onChange: (facets: InboxFacets) => void;
  options: InboxOptions;
  canAssign: boolean;
  names: ReadonlyMap<string, string>;
  client: InboxClient;
  onOpen: (kind: FilterKind) => void;
}) {
  const colors = useColors();
  const styles = useStyles();
  const chevron = Math.round(12 * Math.max(1, Math.min(useTextScale(), CHIP_TEXT_MAX)));
  const place = (id: string) => placeName(id, options, names);
  const shown: FilterKind[] = [
    "place",
    ...(options.authors.length > 0 || options.me !== undefined || facets.authors?.length
      ? (["author"] as const)
      : []),
    "status",
    "kind",
    "updated",
    ...(options.sources.length > 1 || facets.sources?.length ? (["source"] as const) : []),
  ];
  const onlyPlace = facets.places?.length === 1 ? facets.places[0] : undefined;
  const onlyAuthor = facets.authors?.length === 1 ? facets.authors[0] : undefined;
  const leading = (kind: FilterKind) => {
    if (kind === "place" && onlyPlace && isProject(onlyPlace, options, names)) {
      return <ProjectMascot project={onlyPlace} color={projectColor(onlyPlace)} size={16} />;
    }
    if (kind === "author" && onlyAuthor && onlyAuthor !== ME) {
      const person = options.authors.find((author) => author.name === onlyAuthor);
      return (
        <Avatar
          person={person ?? { name: onlyAuthor }}
          itemKey={person?.itemKey ?? ""}
          client={person ? client : undefined}
          size={18}
        />
      );
    }
    if (kind === "source" && facets.sources?.length === 1) {
      return <SourceLogo source={facets.sources[0]} size={14} />;
    }
    return undefined;
  };
  const active = shown.some((kind) => isSet(kind, facets)) || !!facets.assigned;
  return (
    <>
      {active ? (
        <Pressable
          accessibilityRole="button"
          accessibilityLabel="Clear filters"
          hitSlop={6}
          onPress={() => {
            void Haptics.selectionAsync();
            onChange({});
          }}
          style={[styles.chip, styles.clear]}
        >
          <Icon name="close" size={14} color={colors.muted} />
        </Pressable>
      ) : null}
      {shown.map((kind) => {
        const selected = isSet(kind, facets);
        return (
          <Chip
            key={kind}
            label={filterLabel(kind, facets, place)}
            selected={selected}
            onPress={() => onOpen(kind)}
            leading={leading(kind)}
            trailing={
              <Icon
                name="chevronDown"
                size={chevron}
                color={selected ? colors.accent : colors.muted}
              />
            }
            hint={`Choose ${TITLES[kind].toLowerCase()}`}
          />
        );
      })}
      {canAssign ? (
        <Chip
          label="Assigned to me"
          selected={!!facets.assigned}
          onPress={() => onChange({ ...facets, assigned: !facets.assigned })}
        />
      ) : null}
    </>
  );
}

type SheetRow = {
  id: string;
  label: string;
  selected: boolean;
  onPress: () => void;
  leading?: ReactNode;
  count?: number;
};
type SheetSection = { title?: string; rows: SheetRow[] };

/**
 * One filter's choices in a sheet that rises only as far as they need, up to
 * about 60% of the screen, and on to full height for a list too long for that.
 * A filter that takes several stays up while they're ticked; one that takes a
 * single choice closes on it.
 */
export function FilterSheet({
  open,
  onClose,
  facets,
  onChange,
  options,
  names,
  pinned,
  client,
}: {
  open: FilterKind | null;
  onClose: () => void;
  facets: InboxFacets;
  onChange: (facets: InboxFacets) => void;
  options: InboxOptions;
  names: ReadonlyMap<string, string>;
  /** The project the Inbox was opened from, listed first. */
  pinned?: string;
  client: InboxClient;
}) {
  const colors = useColors();
  const sheetColor = useSheetColor();
  const styles = useStyles();
  const { width, height } = useWindowDimensions();
  const textScale = Math.max(1, useTextScale());
  const { bottom } = useSafeAreaInsets();
  // Stays on the last filter while the sheet slides away.
  const [kind, setKind] = useState<FilterKind>("place");
  if (open && open !== kind) setKind(open);

  const pick = (next: InboxFacets) => {
    void Haptics.selectionAsync();
    onChange(next);
  };
  const pickOnce = (next: InboxFacets) => {
    pick(next);
    onClose();
  };

  const sections = sheetSections(kind, {
    facets,
    options,
    names,
    pinned,
    client,
    colors,
    pick,
    pickOnce,
  });
  const rows = sections.reduce((sum, section) => sum + section.rows.length, 0);
  const headings = sections.filter((section) => section.title).length;
  // Rows grow with the text; a guess over is only a sheet that can rise further.
  const rowHeight = Math.max(SHEET_ROW, ROW_LINE * textScale + ROW_PADDING * 2);
  const fits =
    SHEET_TOP + TITLE * textScale + rows * rowHeight + headings * HEADING * textScale + FOOT;
  const sheetHeight = Math.min(fits, Math.round(height * SHEET_MOST));

  return (
    <BottomSheet
      isPresented={!!open}
      onDismiss={onClose}
      snapPoints={fits > sheetHeight ? [{ height: sheetHeight }, "full"] : [{ height: sheetHeight }]}
      contentPadding={0}
      containerColor={sheetColor}
      modifiers={SHEET_FILL}
    >
      <RNHostView>
        <SheetBody width={width} style={styles.sheet}>
          <SheetTitle title={TITLES[kind]} />
          <SheetFade>
            <ScrollView
              style={styles.rows}
              showsVerticalScrollIndicator={false}
              contentContainerStyle={[
                styles.list,
                { paddingBottom: Math.max(bottom, SHEET_FADE) + FOOT },
              ]}
            >
              {sections.map((section, index) => (
                <View key={section.title ?? index}>
                  {section.title ? <SheetHeading>{section.title}</SheetHeading> : null}
                  <SheetCard>
                    {section.rows.map((row, at) => (
                      <View key={row.id}>
                        {at > 0 ? (
                          <SheetDivider
                            inset={row.leading ? SHEET_INSET + LEADING + 12 : SHEET_INSET}
                          />
                        ) : null}
                        <Pressable
                          accessibilityRole="checkbox"
                          accessibilityLabel={row.label}
                          accessibilityState={{ checked: row.selected }}
                          onPress={row.onPress}
                          style={({ pressed }) => [styles.row, pressed && styles.rowPressed]}
                        >
                          {row.leading ? <View style={styles.rowLeading}>{row.leading}</View> : null}
                          <Text numberOfLines={2} style={styles.rowLabel}>
                            {row.label}
                          </Text>
                          {row.selected ? (
                            <Icon name="check" size={20} color={colors.accent} />
                          ) : row.count !== undefined ? (
                            <Text style={styles.rowCount}>{row.count}</Text>
                          ) : null}
                        </Pressable>
                      </View>
                    ))}
                  </SheetCard>
                </View>
              ))}
            </ScrollView>
          </SheetFade>
        </SheetBody>
      </RNHostView>
    </BottomSheet>
  );
}

function sheetSections(
  kind: FilterKind,
  context: {
    facets: InboxFacets;
    options: InboxOptions;
    names: ReadonlyMap<string, string>;
    pinned?: string;
    client: InboxClient;
    colors: Palette;
    pick: (next: InboxFacets) => void;
    pickOnce: (next: InboxFacets) => void;
  },
): SheetSection[] {
  const { facets, options, names, pinned, client, colors, pick, pickOnce } = context;
  const glyph = (name: IconName, color = colors.muted) => (
    <Icon name={name} size={20} color={color} />
  );
  switch (kind) {
    case "place": {
      const selected = facets.places ?? [];
      const row = (id: string, label: string, count?: number): SheetRow => ({
        id,
        label,
        count,
        selected: selected.includes(id),
        onPress: () => pick({ ...facets, places: toggled(facets.places, id) }),
        leading: isProject(id, options, names) ? (
          <ProjectMascot project={id} color={projectColor(id)} size={22} />
        ) : id.startsWith("linear:") ? (
          <SourceLogo source="linear" size={20} />
        ) : (
          glyph("folder")
        ),
      });
      const of = (wanted: InboxOptions["places"][number]["kind"]) =>
        options.places
          .filter((place) => place.kind === wanted)
          .map((place) => row(place.id, names.get(place.id) ?? place.name, place.count));
      const projects = of("project").sort(
        (a, b) => Number(b.id === pinned) - Number(a.id === pinned),
      );
      // A choice stays listed after the scope changes and leaves it with no items.
      const missing = selected
        .filter((id) => !options.places.some((place) => place.id === id))
        .map((id) => row(id, placeName(id, options, names)));
      return [
        {
          rows: [
            {
              id: "any",
              label: "All projects",
              leading: glyph("layers"),
              selected: selected.length === 0,
              onPress: () => pick({ ...facets, places: [] }),
            },
            ...projects,
            ...missing,
          ],
        },
        { title: "Teams and trackers", rows: of("tracker") },
        { title: "Linear projects", rows: of("linearProject") },
      ].filter((section) => section.rows.length > 0);
    }
    case "author": {
      const selected = facets.authors ?? [];
      const row = (
        id: string,
        label: string,
        leading: ReactNode,
        count?: number,
      ): SheetRow => ({
        id,
        label,
        count,
        leading,
        selected: selected.some((author) => author.toLowerCase() === id.toLowerCase()),
        onPress: () => pick({ ...facets, authors: toggled(facets.authors, id) }),
      });
      const people = options.authors.map((person) =>
        row(
          person.name,
          person.name,
          <Avatar person={person} itemKey={person.itemKey} client={client} size={26} />,
          person.count,
        ),
      );
      const missing = selected
        .filter((id) => id !== ME && !people.some((person) => person.id === id))
        .map((id) => row(id, id, <Avatar person={{ name: id }} itemKey="" size={26} />));
      return [
        {
          rows: [
            {
              id: "any",
              label: "Anyone",
              leading: glyph("people"),
              selected: selected.length === 0,
              onPress: () => pick({ ...facets, authors: [] }),
            },
            ...(options.me !== undefined
              ? [row(ME, "Me", <Avatar person={{ name: "Me" }} itemKey="" size={26} />, options.me)]
              : []),
            ...people,
            ...missing,
          ],
        },
      ];
    }
    case "status":
      return [
        {
          rows: [
            {
              id: "any",
              label: "Any status",
              leading: glyph("layers"),
              selected: !facets.statuses?.length,
              onPress: () => pick({ ...facets, statuses: [] }),
            },
            ...STATUSES.map((status) => {
              const { icon, tint } = itemState({ kind: "pr", status: status.id }, colors);
              return {
                ...status,
                leading: glyph(icon, tint),
                selected: !!facets.statuses?.includes(status.id),
                onPress: () => pick({ ...facets, statuses: toggled(facets.statuses, status.id) }),
              };
            }),
          ],
        },
      ];
    case "source":
      return [
        {
          rows: [
            {
              id: "any",
              label: "All sources",
              leading: glyph("layers"),
              selected: !facets.sources?.length,
              onPress: () => pick({ ...facets, sources: [] }),
            },
            ...[...new Set([...options.sources, ...(facets.sources ?? [])])].map((source) => ({
              id: source,
              label: SOURCE_NAMES[source] ?? source,
              leading: <SourceLogo source={source} size={20} />,
              selected: !!facets.sources?.includes(source),
              onPress: () => pick({ ...facets, sources: toggled(facets.sources, source) }),
            })),
          ],
        },
      ];
    case "kind":
      return [
        {
          rows: [
            {
              id: "any",
              label: "All types",
              leading: glyph("layers"),
              selected: !facets.kind,
              onPress: () => pickOnce({ ...facets, kind: undefined }),
            },
            ...KINDS.map((entry) => ({
              ...entry,
              leading: glyph(entry.id === "pr" ? "pullRequest" : "issue"),
              selected: facets.kind === entry.id,
              onPress: () => pickOnce({ ...facets, kind: entry.id }),
            })),
          ],
        },
      ];
    case "updated":
      return [
        {
          rows: [
            {
              id: "any",
              label: "Any time",
              selected: !facets.updated,
              onPress: () => pickOnce({ ...facets, updated: undefined }),
            },
            ...TIMES.map((entry) => ({
              ...entry,
              selected: facets.updated === entry.id,
              onPress: () => pickOnce({ ...facets, updated: entry.id }),
            })),
          ],
        },
      ];
  }
}

const useStyles = themedStyles((colors) => ({
  chip: {
    minHeight: 32,
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
    paddingVertical: 4,
    paddingHorizontal: 12,
    borderRadius: 22,
    backgroundColor: colors.fill,
  },
  chipOn: {
    backgroundColor: withAlpha(colors.accent, 0.18),
  },
  chipLabel: {
    color: colors.muted,
    fontSize: 14,
    fontWeight: "500",
  },
  chipLabelOn: {
    color: colors.accent,
  },
  chipCount: {
    color: colors.muted,
    fontSize: 13,
    fontVariant: ["tabular-nums"],
  },
  clear: {
    width: 32,
    paddingHorizontal: 0,
    justifyContent: "center",
  },
  sheet: {
    paddingTop: SHEET_TOP,
  },
  rows: {
    flex: 1,
  },
  list: {
    paddingTop: SHEET_FADE,
    paddingHorizontal: SHEET_INSET,
  },
  row: {
    minHeight: SHEET_ROW - 1,
    flexDirection: "row",
    alignItems: "center",
    gap: 12,
    paddingVertical: ROW_PADDING,
    paddingHorizontal: SHEET_INSET,
  },
  rowPressed: {
    backgroundColor: colors.fill,
  },
  rowLeading: {
    width: LEADING,
    alignItems: "center",
  },
  rowLabel: {
    flex: 1,
    color: colors.content,
    fontSize: 16,
  },
  rowCount: {
    color: colors.muted,
    fontSize: 15,
    fontVariant: ["tabular-nums"],
  },
}));
