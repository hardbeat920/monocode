import { router, useLocalSearchParams } from "expo-router";
import { Fragment } from "react";
import { Alert, Linking, StyleSheet, Switch, Text, View } from "react-native";

import { useConnection } from "@/features/connection/ConnectionProvider";
import { MONOS_ENABLED } from "@/features/connection/selection";
import {
  liveActivitiesAvailable,
  useLiveActivityPreference,
} from "@/features/liveActivity/liveActivity";
import { useNotificationsBlocked } from "@/features/notifications/SessionNotifications";
import { useAppearancePreference } from "@/features/settings/appearance";
import { accessMode } from "@/features/models/access";
import {
  useDefaultModel,
  useDefaultRuntimeMode,
} from "@/features/settings/defaultModel";
import {
  NOTIFICATION_CATEGORIES,
  useNotificationPreferences,
  type NotificationCategory,
} from "@/features/settings/notifications";
import { useOpenSessionAfterStarting } from "@/features/settings/newSessions";
import { SettingsPage, SettingsRow } from "@/features/settings/SettingsUI";
import { projectForRoute } from "@/features/workspace/routing";
import { useWorkspace } from "@/features/workspace/WorkspaceProvider";
import { Icon, type IconName } from "@/ui/Icon";
import { radii, themedStyles, THEMES, useColors, useTheme } from "@/ui/theme";

const CATEGORY_ICONS: Record<NotificationCategory, IconName> = {
  agentInput: "lock",
  agentFinished: "check",
};

export default function SettingsScreen() {
  const colors = useColors();
  const styles = useStyles();
  // The project open under settings, whose chat background it sets.
  const { projectId } = useLocalSearchParams<{ projectId?: string }>();
  const project = projectForRoute(useWorkspace().projects, projectId);
  const [appearance] = useAppearancePreference();
  const theme = THEMES[useTheme()].label;
  const defaultModel = useDefaultModel();
  const defaultAccess = useDefaultRuntimeMode();
  const notifications = useNotificationPreferences();
  const [openAfterStarting, setOpenAfterStarting] = useOpenSessionAfterStarting();
  const blocked = useNotificationsBlocked();
  const [liveActivity, setLiveActivity] = useLiveActivityPreference();
  // Both pages act on the project open under settings.
  const params = project ? { projectId: project.id } : {};
  const connection = useConnection();
  const selected = connection.selection;
  const disconnect = () =>
    Alert.alert(
      "Disconnect desktop?",
      "You can pair again by scanning a new code. Your workspace choices will be remembered.",
      [
        { text: "Cancel", style: "cancel" },
        {
          text: "Disconnect",
          style: "destructive",
          onPress: () => {
            void connection
              .disconnect()
              .then(() => router.replace("/"))
              .catch((reason) =>
                Alert.alert("Could not disconnect", String(reason)),
              );
          },
        },
      ],
    );
  return (
    <SettingsPage>
      <View style={styles.desktop}>
        <Icon
          name={connection.connected ? "computer" : "computerOff"}
          size={22}
          color={connection.connected ? colors.content : colors.muted}
        />
        <View style={styles.desktopText}>
          <Text style={styles.desktopName} numberOfLines={1}>
            {connection.pairing?.desktop.name}
          </Text>
          <Text style={styles.desktopStatus}>
            {connection.connected
              ? "Connected to desktop"
              : "Desktop unavailable. Your saved workspace stays here."}
          </Text>
        </View>
      </View>

      <Text style={styles.heading}>Notifications</Text>
      <View style={styles.card}>
        {NOTIFICATION_CATEGORIES.map((category, index) => (
          <Fragment key={category.id}>
            {index > 0 ? <View style={styles.divider} /> : null}
            <SettingsRow
              icon={CATEGORY_ICONS[category.id]}
              label={category.label}
              onPress={() =>
                notifications.setEnabled(
                  category.id,
                  !notifications.enabled(category.id),
                )
              }
              accessory={
                <Switch
                  value={notifications.enabled(category.id)}
                  onValueChange={(on) =>
                    notifications.setEnabled(category.id, on)
                  }
                  trackColor={{ true: colors.accent }}
                />
              }
            />
          </Fragment>
        ))}
        {liveActivitiesAvailable() ? (
          <>
            <View style={styles.divider} />
            <SettingsRow
              icon="metrics"
              label="Live Activity"
              onPress={() => setLiveActivity(!liveActivity)}
              accessory={
                <Switch
                  value={liveActivity}
                  onValueChange={setLiveActivity}
                  trackColor={{ true: colors.accent }}
                />
              }
            />
          </>
        ) : null}
      </View>
      {blocked ? (
        <Text style={styles.note} onPress={() => void Linking.openSettings()}>
          Notifications are off for this app in system settings.{" "}
          <Text style={{ color: colors.accent }}>Open Settings</Text>
        </Text>
      ) : null}

      <Text style={styles.heading}>Preferences</Text>
      <View style={styles.card}>
        <SettingsRow
          icon="appearance"
          label="Appearance"
          value={appearance === "system" ? theme : `${theme}, ${appearance}`}
          onPress={() =>
            router.push({ pathname: "/settings/appearance", params })
          }
        />
        <View style={styles.divider} />
        <SettingsRow
          icon="robot"
          label="Providers"
          value={defaultModel?.name ?? "Desktop default"}
          onPress={() =>
            router.push({ pathname: "/settings/providers", params })
          }
        />
        <View style={styles.divider} />
        <SettingsRow
          icon="lock"
          label="Permissions"
          value={
            defaultAccess ? accessMode(defaultAccess).label : "Desktop default"
          }
          onPress={() =>
            router.push({ pathname: "/settings/permissions", params })
          }
        />
      </View>

      <Text style={styles.heading}>New sessions</Text>
      <View style={styles.card}>
        <SettingsRow
          icon="chat"
          label="Open after starting"
          onPress={() => setOpenAfterStarting(!openAfterStarting)}
          accessory={
            <Switch
              accessibilityLabel="Open sessions after starting"
              value={openAfterStarting}
              onValueChange={setOpenAfterStarting}
              trackColor={{ true: colors.accent }}
            />
          }
        />
      </View>
      <Text style={styles.note}>
        Turn off to stay on the new session screen and start more sessions.
      </Text>

      <Text style={styles.heading}>Workspace</Text>
      <View style={styles.card}>
        <SettingsRow
          icon="folder"
          label={MONOS_ENABLED ? "Projects and monos" : "Projects"}
          value={String(
            (selected?.projectIds.length ?? 0) +
              (MONOS_ENABLED ? (selected?.monoIds.length ?? 0) : 0),
          )}
          onPress={() => router.push("/settings/workspace")}
        />
        <View style={styles.divider} />
        <SettingsRow
          icon="computer"
          label="Reconnect"
          showChevron={false}
          onPress={() => void connection.refresh()}
        />
      </View>

      <View style={[styles.card, styles.lastCard]}>
        <SettingsRow
          icon="computerOff"
          label="Disconnect desktop"
          showChevron={false}
          destructive
          onPress={disconnect}
        />
      </View>

      {connection.error ? (
        <Text style={styles.error}>{connection.error}</Text>
      ) : null}
    </SettingsPage>
  );
}

const useStyles = themedStyles((colors) => ({
  desktop: {
    flexDirection: "row",
    alignItems: "center",
    gap: 14,
    padding: 16,
    borderRadius: radii.card,
    borderWidth: StyleSheet.hairlineWidth,
    borderColor: colors.stroke,
    backgroundColor: colors.surface,
  },
  desktopText: {
    flex: 1,
    gap: 2,
  },
  desktopName: {
    color: colors.content,
    fontSize: 17,
    fontWeight: "600",
  },
  desktopStatus: {
    color: colors.muted,
    fontSize: 14,
    lineHeight: 20,
  },
  // Grouped as iOS settings are: a muted heading over a rounded card.
  heading: {
    color: colors.muted,
    fontSize: 15,
    marginTop: 24,
    marginBottom: 8,
    marginLeft: 16,
  },
  card: {
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
  note: {
    color: colors.muted,
    fontSize: 13,
    lineHeight: 18,
    marginTop: 8,
    marginHorizontal: 16,
  },
  lastCard: {
    marginTop: 24,
  },
  error: {
    color: colors.deletions,
    fontSize: 14,
    lineHeight: 21,
    marginTop: 16,
  },
}));
