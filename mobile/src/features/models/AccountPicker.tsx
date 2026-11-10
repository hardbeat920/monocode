import * as Haptics from "expo-haptics";
import { useState } from "react";
import {
  ActivityIndicator,
  Pressable,
  StyleSheet,
  Text,
  View,
} from "react-native";

import { Icon } from "@/ui/Icon";
import { themedStyles, useColors, withAlpha } from "@/ui/theme";

import type { ModelAccount } from "./types";
import type { AccountUsageSnapshot, useAccountUsage } from "./useAccountUsage";

/** The selected account when folded; one compact row per account when open. */
export function AccountPicker({
  accounts,
  value,
  existingSession,
  onPick,
  usage,
}: {
  accounts: ModelAccount[];
  value: string;
  existingSession: boolean;
  onPick: (accountId: string) => void;
  usage: ReturnType<typeof useAccountUsage>;
}) {
  const colors = useColors();
  const styles = useStyles();
  const [open, setOpen] = useState(false);
  const selected = accounts.find((account) => account.id === value);
  const selectedUsage = selected ? usage.accounts[selected.id] : undefined;

  return (
    <View style={styles.card}>
      <View style={styles.header}>
        <Pressable
          accessibilityRole="button"
          accessibilityLabel={`Account: ${selected?.label ?? "Account unavailable"}`}
          accessibilityState={{ expanded: open }}
          onPress={() => setOpen((current) => !current)}
          style={({ pressed }) => [styles.toggle, pressed && styles.pressed]}
        >
          <Text style={styles.selectedName} numberOfLines={1}>
            {selected?.label ?? "Unavailable"}
          </Text>
          <AccountWarning snapshot={selectedUsage} />
          <Icon
            name={open ? "chevronDown" : "chevronRight"}
            size={14}
            color={colors.muted}
          />
        </Pressable>
        <Pressable
          accessibilityRole="button"
          accessibilityLabel="Refresh account usage"
          accessibilityState={{
            disabled: usage.refreshing,
            busy: usage.refreshing,
          }}
          disabled={usage.refreshing}
          onPress={usage.refresh}
          style={({ pressed }) => [styles.refresh, pressed && styles.pressed]}
        >
          {usage.refreshing ? (
            <ActivityIndicator size="small" color={colors.muted} />
          ) : (
            <Icon name="refresh" size={17} color={colors.muted} />
          )}
        </Pressable>
      </View>

      {usage.error ? <Text style={styles.note}>{usage.error}</Text> : null}

      {open ? (
        <View>
          {accounts.map((account, index) => {
            const checked = account.id === value;
            const snapshot = usage.accounts[account.id];
            return (
              <View key={account.id}>
                {index > 0 ? <View style={styles.divider} /> : null}
                <Pressable
                  accessibilityRole="radio"
                  accessibilityState={{ checked }}
                  accessibilityLabel={account.label}
                  onPress={() => {
                    if (checked) return;
                    void Haptics.selectionAsync();
                    onPick(account.id);
                    setOpen(false);
                  }}
                  style={({ pressed }) => [
                    styles.account,
                    pressed && styles.pressed,
                  ]}
                >
                  <View style={styles.accountTitle}>
                    <Text
                      style={[styles.name, checked && styles.selected]}
                      numberOfLines={1}
                    >
                      {account.label}
                    </Text>
                    <AccountWarning snapshot={snapshot} />
                    {checked ? (
                      <Icon name="check" size={17} color={colors.accent} />
                    ) : null}
                  </View>
                  <AccountUsage
                    snapshot={snapshot}
                    loading={usage.refreshing}
                    failed={!!usage.error}
                  />
                </Pressable>
              </View>
            );
          })}
          {existingSession || accounts.length < 2 ? (
            <Text style={styles.note}>
              {existingSession
                ? "Switching accounts opens a new chat."
                : "Add accounts in desktop Settings → Accounts."}
            </Text>
          ) : null}
        </View>
      ) : selected ? (
        <View style={styles.summary}>
          <AccountUsage
            snapshot={selectedUsage}
            loading={usage.refreshing}
            failed={!!usage.error}
          />
        </View>
      ) : null}
    </View>
  );
}

/** Healthy accounts need no extra status label; warnings stay beside the name. */
function AccountWarning({
  snapshot,
}: {
  snapshot: AccountUsageSnapshot | undefined;
}) {
  const colors = useColors();
  const styles = useStyles();
  const tone = snapshot?.status.tone;
  if (!snapshot || (tone !== "low" && tone !== "exhausted")) return null;
  return (
    <Text
      style={[
        styles.warning,
        { color: tone === "low" ? colors.caution : colors.deletions },
      ]}
    >
      {snapshot.status.label}
    </Text>
  );
}

/** A window, reset countdown and percentage share one line above its meter. */
function AccountUsage({
  snapshot,
  loading,
  failed,
}: {
  snapshot: AccountUsageSnapshot | undefined;
  loading: boolean;
  failed: boolean;
}) {
  const colors = useColors();
  const styles = useStyles();
  if (!snapshot || snapshot.meters.length === 0)
    return (
      <Text style={styles.meta}>
        {snapshot?.status.label ??
          (loading
            ? "Checking usage…"
            : failed
              ? "Usage unavailable"
              : "No usage data")}
      </Text>
    );

  return (
    <View style={styles.usage}>
      {snapshot.meters.map((meter) => {
        const color =
          meter.usedPercent >= 90
            ? colors.deletions
            : meter.usedPercent >= 80
              ? colors.caution
              : colors.muted;
        const reset = /^Resets in |^reset due/.test(meter.reset)
          ? ` · ${meter.reset.replace(/^Resets in /, "")}`
          : "";
        return (
          <View key={meter.title} style={styles.meter}>
            <View style={styles.meterLabels}>
              <Text
                style={[styles.meta, styles.window]}
                numberOfLines={1}
                accessibilityLabel={`${meter.title}, ${meter.reset}`}
              >
                {meter.title}
                {reset}
              </Text>
              <Text style={[styles.percent, { color }]}>{meter.label}</Text>
            </View>
            <View
              accessibilityRole="progressbar"
              accessibilityLabel={`${meter.title} usage`}
              accessibilityValue={{
                min: 0,
                max: 100,
                now: meter.percent,
                text: meter.label,
              }}
              style={styles.track}
            >
              <View
                style={[
                  styles.fill,
                  { width: `${meter.percent}%`, backgroundColor: color },
                ]}
              />
            </View>
          </View>
        );
      })}
      {snapshot.stale && !failed ? (
        <Text style={styles.meta}>Last known usage</Text>
      ) : null}
    </View>
  );
}

const useStyles = themedStyles((colors) => ({
  card: {
    paddingHorizontal: 14,
    borderRadius: 18,
    borderCurve: "continuous",
    backgroundColor: withAlpha(colors.content, 0.07),
  },
  header: { flexDirection: "row", alignItems: "center", gap: 4 },
  toggle: {
    flex: 1,
    minHeight: 44,
    flexDirection: "row",
    alignItems: "center",
    gap: 8,
  },
  refresh: {
    width: 44,
    height: 44,
    marginRight: -10,
    alignItems: "center",
    justifyContent: "center",
  },
  selectedName: {
    color: colors.content,
    fontSize: 15,
    fontWeight: "500",
    flexShrink: 1,
  },
  summary: { paddingBottom: 14 },
  account: { minHeight: 44, paddingTop: 10, paddingBottom: 14, gap: 8 },
  accountTitle: { flexDirection: "row", alignItems: "center", gap: 8 },
  name: { flex: 1, color: colors.content, fontSize: 15 },
  selected: { fontWeight: "600" },
  warning: { fontSize: 11, flexShrink: 0 },
  usage: { gap: 8 },
  meter: { gap: 5 },
  meterLabels: {
    flexDirection: "row",
    alignItems: "baseline",
    justifyContent: "space-between",
    gap: 8,
  },
  meta: { color: colors.muted, fontSize: 12, lineHeight: 16 },
  window: { flex: 1 },
  percent: { fontSize: 12, lineHeight: 16, fontVariant: ["tabular-nums"] },
  track: {
    height: 3,
    borderRadius: 2,
    overflow: "hidden",
    backgroundColor: withAlpha(colors.content, 0.1),
  },
  fill: { height: 3, borderRadius: 2 },
  note: {
    color: colors.muted,
    fontSize: 12,
    lineHeight: 16,
    paddingBottom: 12,
  },
  divider: { height: StyleSheet.hairlineWidth, backgroundColor: colors.stroke },
  pressed: { opacity: 0.5 },
}));
