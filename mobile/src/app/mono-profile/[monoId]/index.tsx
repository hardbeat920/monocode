import { router, useLocalSearchParams } from "expo-router";
import { Fragment } from "react";
import { ActivityIndicator, Pressable, StyleSheet, Text, View } from "react-native";

import { PixelMascot } from "@/features/mascots/PixelMascot";
import { ProjectMascot } from "@/features/mascots/ProjectMascot";
import { useMonoModel } from "@/features/models/useMonoModel";
import { ContributionGraph } from "@/features/visuals/charts";
import { ProviderLogo } from "@/features/sessions/ProviderLogo";
import { SettingsPage, SettingsRow } from "@/features/settings/SettingsUI";
import { useMonoProfile, type MonoProfile } from "@/features/workspace/monoProfile";
import { useWorkspace } from "@/features/workspace/WorkspaceProvider";
import { Icon } from "@/ui/Icon";
import { radii, themedStyles, useColors } from "@/ui/theme";

// A Mono's profile, as the desktop's details sidebar has it without its
// Changes: who it is and the model it runs on, with Edit under, then its day,
// its contributions, what it does next, the documents it wrote and the
// projects it works in.
export default function MonoProfileScreen() {
  const { monoId } = useLocalSearchParams<{ monoId: string }>();
  const { monos, allProjects } = useWorkspace();
  const mono = monos.find((m) => m.id === monoId);
  const { profile, error, refreshing, refresh } = useMonoProfile(monoId);
  const { model, harness, effort } = useMonoModel(mono);
  const styles = useStyles();
  const colors = useColors();
  if (!mono) return null;
  const assigned = allProjects.filter((project) => mono.projectIds.includes(project.id));
  return (
    <SettingsPage onRefresh={refresh} refreshing={refreshing}>
      <View style={styles.identity}>
        <PixelMascot name={mono.mascot} color={mono.color} status={mono.status} size={88} />
        <Text style={styles.name} numberOfLines={1}>{mono.name}</Text>
        {model ? (
          <View style={styles.model}>
            {harness ? <ProviderLogo harness={harness} size={15} /> : null}
            <Text style={styles.status} numberOfLines={1}>
              {model}
              {effort ? <Text style={styles.effort}> {effort}</Text> : null}
            </Text>
          </View>
        ) : null}
        <Pressable
          accessibilityRole="button"
          onPress={() => router.push({ pathname: "/mono-profile/[monoId]/edit", params: { monoId } })}
          style={({ pressed }) => [styles.edit, pressed && styles.pressed]}
        >
          <Text style={styles.editLabel}>Edit</Text>
        </Pressable>
      </View>

      {profile ? (
        <Overview profile={profile} name={mono.name} monoId={monoId} />
      ) : error ? (
        <Text style={styles.note}>{error}</Text>
      ) : (
        <ActivityIndicator color={colors.muted} style={styles.loading} />
      )}

      <Text style={styles.heading}>Projects</Text>
      <View style={styles.card}>
        {assigned.map((project) => (
          <Fragment key={project.id}>
            <SettingsRow
              icon={<ProjectMascot project={project.id} color={project.color} name={project.mascot} />}
              label={project.name}
              showChevron={false}
            />
            <View style={styles.divider} />
          </Fragment>
        ))}
        {/* Last in the list: picks which of the desktop's projects it works in. */}
        <Pressable
          accessibilityRole="button"
          onPress={() => router.push({ pathname: "/mono-profile/[monoId]/projects", params: { monoId } })}
          style={({ pressed }) => [styles.addRow, pressed && styles.pressed]}
        >
          <View style={styles.addIcon}>
            <Icon name="add" size={18} color={colors.accent} />
          </View>
          <Text style={styles.addLabel}>{assigned.length ? "Assign projects" : "Assign a project"}</Text>
        </Pressable>
      </View>
      {assigned.length ? null : (
        <Text style={styles.note}>Without a project it works from your home folder.</Text>
      )}
    </SettingsPage>
  );
}

function Overview({ profile, name, monoId }: { profile: MonoProfile; name: string; monoId: string }) {
  const styles = useStyles();
  const colors = useColors();
  const { today, contributions } = profile;
  // A habit or document opens beside the profile, inside its sheet.
  const openHabit = (habitId: string) =>
    router.push({ pathname: "/mono-profile/[monoId]/habit", params: { monoId, habitId } });
  return (
    <>
      <Text style={styles.heading}>Today</Text>
      <View style={styles.tiles}>
        <Tile value={String(today.merged)} label="PRs merged" />
        <Tile
          value={
            <Text style={styles.figure}>
              <Text style={{ color: colors.additions }}>+{today.additions.toLocaleString()}</Text>
              <Text style={{ color: colors.deletions }}> −{today.deletions.toLocaleString()}</Text>
            </Text>
          }
          label="Lines changed"
        />
        <Tile value={String(today.sessions)} label="Sessions" />
        <Tile value={`${today.habitsRan}/${today.habitsTotal}`} label="Habits run" />
      </View>

      <Text style={styles.heading}>Contributions</Text>
      <View style={[styles.card, styles.padded]}>
        <View style={styles.contributionsLine}>
          <Text style={styles.total}>{contributions.total.toLocaleString()}</Text>
          <Text style={styles.caption}>in the last 6 months</Text>
          {contributions.streak > 1 ? (
            <Text style={[styles.caption, styles.streak]}>{contributions.streak}-day streak</Text>
          ) : null}
        </View>
        <ContributionGraph levels={contributions.levels} />
      </View>

      <Text style={styles.heading}>Up next</Text>
      <View style={styles.card}>
        {profile.upNext.length ? (
          profile.upNext.map((habit, index) => (
            <Fragment key={habit.id}>
              {index > 0 ? <View style={styles.divider} /> : null}
              <Pressable
                accessibilityRole="button"
                onPress={() => openHabit(habit.id)}
                style={({ pressed }) => [styles.listRow, pressed && styles.pressed]}
              >
                <View style={styles.listIcon}>
                  <Icon name={habit.running ? "terminal" : "automation"} size={18} color={colors.content} />
                </View>
                <View style={styles.listText}>
                  <Text style={styles.listTitle} numberOfLines={1}>{habit.name}</Text>
                  <Text style={styles.listDetail} numberOfLines={1}>{habit.schedule}</Text>
                </View>
                <Text style={styles.listDetail}>{habit.when}</Text>
              </Pressable>
            </Fragment>
          ))
        ) : (
          <Text style={styles.empty}>Nothing scheduled. Ask {name} to put recurring work on a habit.</Text>
        )}
      </View>

      {profile.habits.length ? (
        <>
          <Text style={styles.heading}>Habits</Text>
          <View style={styles.card}>
            {profile.habits.map((habit, index) => (
              <Fragment key={habit.id}>
                {index > 0 ? <View style={styles.divider} /> : null}
                <SettingsRow
                  icon="automation"
                  label={habit.name}
                  value={habit.enabled ? habit.schedule : "Paused"}
                  onPress={() => openHabit(habit.id)}
                />
              </Fragment>
            ))}
          </View>
        </>
      ) : null}

      <Text style={styles.heading}>Documents</Text>
      <View style={styles.card}>
        {profile.documents.length ? (
          profile.documents.map((document, index) => (
            <Fragment key={document.id}>
              {index > 0 ? <View style={styles.divider} /> : null}
              <Pressable
                accessibilityRole="button"
                accessibilityLabel={`Open ${document.title}`}
                onPress={() =>
                  router.push({
                    pathname: "/mono-profile/[monoId]/document",
                    params: { monoId, documentId: document.id, title: document.title },
                  })
                }
                style={({ pressed }) => [styles.listRow, pressed && styles.pressed]}
              >
                <View style={styles.listIcon}>
                  <Icon
                    name={["html", "visual"].includes(document.kind) ? "metrics" : "edit"}
                    size={18}
                    color={colors.content}
                  />
                </View>
                <View style={styles.listText}>
                  <Text style={styles.listTitle} numberOfLines={1}>{document.title}</Text>
                </View>
                <Text style={styles.listDetail}>{ago(document.updatedAt)}</Text>
              </Pressable>
            </Fragment>
          ))
        ) : (
          <Text style={styles.empty}>No documents yet.</Text>
        )}
      </View>
    </>
  );
}

function Tile({ value, label }: { value: string | React.ReactElement; label: string }) {
  const styles = useStyles();
  return (
    <View style={styles.tile}>
      {typeof value === "string" ? <Text style={styles.figure}>{value}</Text> : value}
      <Text style={styles.caption}>{label}</Text>
    </View>
  );
}

/** How long ago, in the desktop's short form: now, 5m, 3h, 2d. */
function ago(ms: number) {
  const seconds = Math.max(0, (Date.now() - ms) / 1000);
  if (seconds < 60) return "now";
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h`;
  return `${Math.floor(seconds / 86400)}d`;
}

const useStyles = themedStyles((colors) => ({
  // Up under the sheet's header, which has no title of its own here.
  identity: {
    alignItems: "center",
    gap: 8,
    marginTop: -12,
  },
  model: {
    flexDirection: "row",
    alignItems: "center",
    gap: 6,
    paddingHorizontal: 24,
  },
  effort: {
    color: colors.faint,
  },
  // As a settings row, so it lines up with the projects above it.
  addRow: {
    flexDirection: "row",
    alignItems: "center",
    minHeight: 52,
    paddingVertical: 8,
  },
  addIcon: {
    width: 36,
    height: 36,
    marginRight: 4,
    alignItems: "center",
    justifyContent: "center",
  },
  addLabel: {
    color: colors.accent,
    fontSize: 17,
  },
  // Close under the mascot, whose sprite already leaves room around it.
  name: {
    marginTop: -6,
    color: colors.content,
    fontSize: 24,
    fontWeight: "700",
  },
  status: {
    flexShrink: 1,
    color: colors.muted,
    fontSize: 15,
  },
  edit: {
    marginTop: 6,
    paddingHorizontal: 22,
    paddingVertical: 8,
    borderRadius: radii.pill,
    backgroundColor: colors.surface,
  },
  editLabel: {
    color: colors.accent,
    fontSize: 15,
    fontWeight: "600",
  },
  pressed: {
    opacity: 0.6,
  },
  loading: {
    marginTop: 32,
  },
  heading: {
    color: colors.muted,
    fontSize: 15,
    marginTop: 24,
    marginBottom: 8,
    marginLeft: 16,
  },
  note: {
    color: colors.muted,
    fontSize: 13,
    lineHeight: 18,
    marginTop: 16,
    marginHorizontal: 16,
    textAlign: "center",
  },
  card: {
    backgroundColor: colors.surface,
    borderRadius: radii.card,
    borderCurve: "continuous",
    paddingHorizontal: 10,
    paddingVertical: 6,
  },
  padded: {
    paddingHorizontal: 16,
    paddingVertical: 14,
    gap: 12,
  },
  divider: {
    height: StyleSheet.hairlineWidth,
    marginLeft: 40,
    backgroundColor: colors.stroke,
  },
  empty: {
    color: colors.muted,
    fontSize: 14,
    lineHeight: 20,
    paddingHorizontal: 8,
    paddingVertical: 12,
  },
  tiles: {
    flexDirection: "row",
    flexWrap: "wrap",
    gap: 10,
  },
  tile: {
    flexBasis: "47%",
    flexGrow: 1,
    backgroundColor: colors.surface,
    borderRadius: radii.card,
    borderCurve: "continuous",
    paddingHorizontal: 16,
    paddingVertical: 14,
    gap: 4,
  },
  figure: {
    color: colors.content,
    fontSize: 22,
    fontWeight: "600",
    fontVariant: ["tabular-nums"],
  },
  caption: {
    color: colors.muted,
    fontSize: 13,
  },
  contributionsLine: {
    flexDirection: "row",
    alignItems: "baseline",
    gap: 6,
  },
  total: {
    color: colors.content,
    fontSize: 17,
    fontWeight: "600",
    fontVariant: ["tabular-nums"],
  },
  streak: {
    marginLeft: "auto",
  },
  // As a settings row, so habits and documents sit like the projects do.
  listRow: {
    flexDirection: "row",
    alignItems: "center",
    minHeight: 52,
    paddingVertical: 8,
    paddingRight: 8,
  },
  listIcon: {
    width: 36,
    height: 36,
    marginRight: 4,
    alignItems: "center",
    justifyContent: "center",
  },
  listText: {
    flex: 1,
    gap: 2,
  },
  listTitle: {
    color: colors.content,
    fontSize: 17,
  },
  listDetail: {
    color: colors.muted,
    fontSize: 13,
  },
}));
