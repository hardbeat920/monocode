import * as Haptics from "expo-haptics";
import {
  Children,
  Fragment,
  useState,
  type ReactElement,
  type ReactNode,
} from "react";
import {
  ActivityIndicator,
  Platform,
  Pressable,
  ScrollView,
  StyleSheet,
  Text,
  View,
} from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";
import { MonoMascot } from "@/features/mascots/PixelMascot";
import { ProjectMascot } from "@/features/mascots/ProjectMascot";
import { SettingsRow } from "@/features/settings/SettingsUI";
import { GradualBlur } from "@/ui/GradualBlur";
import { Icon } from "@/ui/Icon";
import { LiquidView } from "@/ui/LiquidView";
import { withAlpha, radii, themedStyles, useColors } from "@/ui/theme";
import { useConnection } from "./ConnectionProvider";
import { connectionStyleSheet } from "./ConnectScreen";
import { MONOS_ENABLED, type MobileSelection } from "./selection";

/** Room the blur takes past the edge it covers. */
const FADE = 24;

/**
 * The projects and monos the phone shows, picked as cards; only projects
 * while monos are off (see MONOS_ENABLED). First run shows it
 * on its own; Settings shows it `editing`, in its sheet under a native header,
 * which the page scrolls under through iOS's automatic insets.
 */
export function WorkspacePicker({
  editing = false,
  headerHeight = 0,
  onDone,
}: {
  editing?: boolean;
  headerHeight?: number;
  onDone?: () => void;
}) {
  const colors = useColors();
  const styles = useStyles();
  const connection = useConnection();
  const insets = useSafeAreaInsets();
  const androidHeader = editing && Platform.OS === "android";
  const [draft, setDraft] = useState<MobileSelection>(
    () => connection.selection ?? { projectIds: [], monoIds: [] },
  );
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const [footer, setFooter] = useState(120);
  const background = editing ? colors.sheet : colors.background;
  const monos = MONOS_ENABLED ? connection.catalog.monos : [];
  const toggle = (field: keyof MobileSelection, id: string) => {
    void Haptics.selectionAsync();
    setDraft((previous) => ({
      ...previous,
      [field]: previous[field].includes(id)
        ? previous[field].filter((value) => value !== id)
        : [...previous[field], id],
    }));
  };
  const save = async () => {
    setSaving(true);
    setError("");
    try {
      await connection.select(draft);
      onDone?.();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : String(reason));
    } finally {
      setSaving(false);
    }
  };
  const count =
    draft.projectIds.filter((id) =>
      connection.catalog.projects.some((project) => project.id === id),
    ).length +
    draft.monoIds.filter((id) => monos.some((mono) => mono.id === id)).length;
  const disabled = saving || (!editing && !connection.connected);
  const empty = !connection.catalog.projects.length && !monos.length;
  const content = (
    <>
      <ScrollView
        contentInsetAdjustmentBehavior={
          editing && !androidHeader ? "automatic" : "never"
        }
        showsVerticalScrollIndicator={false}
        contentContainerStyle={{
          paddingTop: editing
            ? (androidHeader ? headerHeight + 12 : 4)
            : insets.top + 48,
          paddingBottom: footer + 16,
          paddingHorizontal: 20,
        }}
      >
        {/* In Settings the header titles it, as on the other pages. */}
        {editing ? null : (
          <>
            <Text style={styles.title}>Your mobile{"\n"}workspace</Text>
            <Text style={styles.subtitle}>
              Pick the {MONOS_ENABLED ? "projects and monos" : "projects"} you
              want here.{"\n"}You can change this anytime in Settings.
            </Text>
          </>
        )}

        {monos.length ? (
          <Section title="Monos" editing={editing}>
            {monos.map((mono) => (
              <PickCard
                key={mono.id}
                editing={editing}
                label={mono.name}
                selected={draft.monoIds.includes(mono.id)}
                onPress={() => toggle("monoIds", mono.id)}
                icon={
                  <MonoMascot
                    name={mono.mascot}
                    color={mono.color}
                    status={mono.status}
                    size={24}
                  />
                }
              />
            ))}
          </Section>
        ) : null}
        {connection.catalog.projects.length ? (
          <Section title="Projects" editing={editing}>
            {connection.catalog.projects.map((project) => (
              <PickCard
                key={project.id}
                editing={editing}
                label={project.name}
                selected={draft.projectIds.includes(project.id)}
                onPress={() => toggle("projectIds", project.id)}
                icon={
                  <ProjectMascot
                    project={project.path}
                    name={project.mascot}
                    color={project.color}
                    size={20}
                  />
                }
              />
            ))}
          </Section>
        ) : null}
        {empty ? (
          connection.connected ? (
            <Text style={styles.empty}>
              Open a project or create a mono on your desktop, then refresh.
            </Text>
          ) : (
            <ActivityIndicator color={colors.content} style={styles.loading} />
          )
        ) : null}
      </ScrollView>

      {editing ? null : (
        <GradualBlur edge="top" height={insets.top + FADE} color={background} />
      )}
      <GradualBlur edge="bottom" height={footer} color={background} />

      <View
        style={[styles.footer, { paddingBottom: insets.bottom + 12 }]}
        onLayout={(event) => setFooter(event.nativeEvent.layout.height + FADE)}
      >
        {error || connection.error ? (
          <Text style={styles.error}>{error || connection.error}</Text>
        ) : null}
        <LiquidView
          onPress={() => void save()}
          disabled={disabled}
          accessibilityLabel="Save"
          tintColor={colors.inverseSurface}
          style={{ opacity: disabled ? 0.5 : 1 }}
          innerStyle={styles.save}
        >
          {saving ? (
            <ActivityIndicator color={colors.inverse} />
          ) : (
            <Text style={styles.saveLabel}>
              {editing
                ? "Save"
                : count
                  ? "Continue with " +
                    count +
                    (count === 1 ? " item" : " items")
                  : "Choose later"}
            </Text>
          )}
        </LiquidView>
        {!connection.connected || !count ? (
          <Pressable hitSlop={8} onPress={() => void connection.refresh()}>
            <Text style={styles.link}>Refresh desktop</Text>
          </Pressable>
        ) : null}
        {editing ? null : (
          <Pressable hitSlop={8} onPress={() => void connection.disconnect()}>
            <Text style={styles.link}>Connect another desktop</Text>
          </Pressable>
        )}
      </View>
    </>
  );
  // Keep the ScrollView at the screen root, like SettingsPage. UIKit then
  // tracks it for transparent navigation and the custom fading header blur.
  return editing ? content : (
    <View style={[styles.screen, { backgroundColor: background }]}>{content}</View>
  );
}

/**
 * A labelled stack of cards; in Settings, a card of rows with dividers
 * between them, as its other pages group theirs.
 */
function Section({
  title,
  editing,
  children,
}: {
  title: string;
  editing: boolean;
  children: ReactNode;
}) {
  const styles = useStyles();
  if (!editing)
    return (
      <View style={styles.section}>
        <Text style={styles.sectionTitle}>{title}</Text>
        {children}
      </View>
    );
  return (
    <>
      <Text style={styles.heading}>{title}</Text>
      <View style={styles.group}>
        {Children.toArray(children).map((child, index) => (
          <Fragment key={index}>
            {index > 0 ? <View style={styles.divider} /> : null}
            {child}
          </Fragment>
        ))}
      </View>
    </>
  );
}

function PickCard({
  label,
  icon,
  selected,
  editing,
  onPress,
}: {
  label: string;
  icon: ReactElement;
  selected: boolean;
  editing: boolean;
  onPress: () => void;
}) {
  const colors = useColors();
  const styles = useStyles();
  if (editing)
    return (
      <SettingsRow
        icon={icon}
        label={label}
        showChevron={false}
        accessory={
          selected ? (
            <Icon name="check" size={18} color={colors.accent} />
          ) : null
        }
        onPress={onPress}
      />
    );
  return (
    <Pressable
      accessibilityRole="checkbox"
      accessibilityLabel={label}
      accessibilityState={{ checked: selected }}
      onPress={onPress}
      style={({ pressed }) => [
        styles.card,
        selected && styles.cardSelected,
        pressed && { opacity: 0.7 },
      ]}
    >
      <View style={styles.cardIcon}>{icon}</View>
      <Text style={styles.cardLabel} numberOfLines={1}>
        {label}
      </Text>
      <View style={[styles.check, selected && styles.checkSelected]}>
        {selected ? (
          <Icon
            name="check"
            size={12}
            color={colors.inverse}
            strokeWidth={2.5}
          />
        ) : null}
      </View>
    </Pressable>
  );
}

const useStyles = themedStyles((colors) => {
  const connectionStyles = connectionStyleSheet(colors);
  return {
    screen: {
      flex: 1,
    },
    title: {
      color: colors.content,
      fontSize: 30,
      lineHeight: 36,
      fontWeight: "600",
      letterSpacing: -0.5,
      textAlign: "center",
    },
    subtitle: {
      color: colors.muted,
      fontSize: 14,
      lineHeight: 20,
      textAlign: "center",
      marginTop: 12,
      marginBottom: 36,
    },
    // Settings' grouping: a muted heading over a rounded card of rows.
    heading: {
      color: colors.muted,
      fontSize: 15,
      marginTop: 20,
      marginBottom: 8,
      marginLeft: 16,
    },
    group: {
      backgroundColor: colors.surface,
      borderRadius: radii.card,
      borderCurve: "continuous",
      paddingHorizontal: 10,
      paddingVertical: 6,
    },
    // Inset to the rows' labels, past their icons.
    divider: {
      height: StyleSheet.hairlineWidth,
      marginLeft: 40,
      backgroundColor: colors.stroke,
    },
    section: {
      gap: 8,
      marginBottom: 32,
    },
    sectionTitle: {
      color: colors.muted,
      fontSize: 13,
      fontWeight: "600",
      marginLeft: 4,
      marginBottom: 2,
    },
    card: {
      flexDirection: "row",
      alignItems: "center",
      gap: 14,
      paddingVertical: 12,
      paddingLeft: 14,
      paddingRight: 16,
      borderRadius: radii.pill,
      backgroundColor: withAlpha(colors.content, 0.04),
    },
    cardSelected: {
      backgroundColor: withAlpha(colors.content, 0.08),
    },
    check: {
      width: 22,
      height: 22,
      borderRadius: 11,
      borderWidth: 1.5,
      borderColor: colors.faint,
      alignItems: "center",
      justifyContent: "center",
    },
    checkSelected: {
      borderColor: colors.content,
      backgroundColor: colors.content,
    },
    cardIcon: {
      width: 28,
      alignItems: "center",
    },
    cardLabel: {
      flex: 1,
      color: colors.content,
      fontSize: 17,
    },
    empty: {
      ...connectionStyles.subtitle,
      textAlign: "center",
    },
    loading: {
      marginTop: 24,
    },
    footer: {
      position: "absolute",
      left: 0,
      right: 0,
      bottom: 0,
      alignItems: "center",
      gap: 4,
      paddingHorizontal: 20,
    },
    error: {
      ...connectionStyles.error,
      textAlign: "center",
    },
    save: {
      height: 52,
      minWidth: 200,
      paddingHorizontal: 32,
      justifyContent: "center",
    },
    saveLabel: {
      color: colors.inverse,
      fontSize: 17,
      fontWeight: "600",
    },
    link: {
      ...connectionStyles.secondary,
      padding: 10,
    },
  };
});
