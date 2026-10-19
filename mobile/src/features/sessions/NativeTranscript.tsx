import { requireNativeView } from "expo";
import * as Clipboard from "expo-clipboard";
import * as Haptics from "expo-haptics";
import { useIsFocused } from "expo-router";
import {
  use,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type Ref,
} from "react";
import {
  Keyboard,
  Linking,
  StyleSheet,
  View,
  useWindowDimensions,
  type NativeSyntheticEvent,
  type ViewProps,
} from "react-native";
import { withTiming } from "react-native-reanimated";

import { useConnection } from "@/features/connection/ConnectionProvider";
import { hasNativeTranscript } from "@/features/connection/nativeLink";
import { pixelLayers } from "@/features/mascots/pixelMascots";
import { useCollapsedToolCalls } from "@/features/settings/toolCalls";
import {
  useNativeTranscript,
  type NativeTranscriptStatus,
} from "@/features/workspace/WorkspaceProvider";
import type { TurnMetrics } from "@/features/workspace/types";
import { useColors } from "@/ui/theme";
import { usePushStarted } from "@/ui/usePushedIn";

import { EmptySession } from "./EmptySession";
import { FileOpener } from "./Markdown";
import { SessionErrorSheet } from "./SessionErrorSheet";
import { useJumpToEnd, useSessionLayout } from "./SessionScreen";
import type { TranscriptAgent } from "./TranscriptTurn";
import { TurnMetricsSheet } from "./TurnMetricsSheet";
import { useReportSessionError, type SessionErrorNotice } from "./sessionError";

type Handle = { scrollToEnd: () => Promise<void> };
type Action = {
  type: "copy" | "metrics" | "error" | "link";
  url?: string;
  copyText?: string;
  metrics?: TurnMetrics;
  durationMs?: number;
  completedAt?: number;
  notice?: SessionErrorNotice;
};
type Props = ViewProps & {
  ref?: Ref<Handle>;
  configuration: string;
  pending: string;
  appearance: string;
  active: boolean;
  topInset: number;
  bottomInset: number;
  composerExtra: number;
  keyboardOffset: number;
  onStatus: (event: NativeSyntheticEvent<NativeTranscriptStatus>) => void;
  onAction: (event: NativeSyntheticEvent<Action>) => void;
  onScrollState: (event: NativeSyntheticEvent<{ away: boolean }>) => void;
};
const NativeView = hasNativeTranscript
  ? requireNativeView<Props>("MonocodeLink")
  : null;

/** React owns chrome and occasional interactions. Native owns all transcript data. */
export function NativeTranscript({
  conversationId,
  agent,
  pushed = false,
}: {
  conversationId: string;
  agent?: TranscriptAgent;
  pushed?: boolean;
}) {
  const { pairing } = useConnection();
  const { pending, onStatus } = useNativeTranscript(conversationId);
  const layout = useSessionLayout();
  const colors = useColors();
  const { fontScale, scale: pixelRatio } = useWindowDimensions();
  const [collapsed] = useCollapsedToolCalls();
  const focused = useIsFocused();
  const started = usePushStarted(pushed);
  const openFile = use(FileOpener);
  const jump = useJumpToEnd();
  const view = useRef<Handle>(null);
  const [notice, setNotice] = useState<SessionErrorNotice | null>(null);
  const [error, setError] = useState<SessionErrorNotice | null>(null);
  const [errorPresented, setErrorPresented] = useState(false);
  const [metrics, setMetrics] = useState<Action | null>(null);
  const [empty, setEmpty] = useState(false);
  const configuration = useMemo(
    () =>
      pairing
        ? JSON.stringify({
            conversationId,
            identity: pairing.identity,
            pin: pairing.desktop.fingerprint,
            addresses: pairing.desktop.addresses,
          })
        : "",
    [pairing, conversationId],
  );
  const appearance = useMemo(
    () =>
      JSON.stringify({
        content: colors.content,
        muted: colors.muted,
        accent: colors.accent,
        bubble: colors.bubble,
        surface: colors.surface,
        surfaceRaised: colors.surfaceRaised,
        stroke: colors.stroke,
        faint: colors.faint,
        fileRef: colors.fileRef,
        fileLinks: !!openFile,
        error: colors.deletions,
        rail: colors.rail,
        agent: agent?.name ?? "",
        agentRects: agent
          ? (() => {
              const layers = pixelLayers(agent.mascot, agent.color);
              return [...layers.body, ...layers.open];
            })()
          : [],
        scale: fontScale,
        pixelRatio,
        collapsed,
      }),
    [colors, agent, openFile, fontScale, pixelRatio, collapsed],
  );
  const showError = useCallback((value: SessionErrorNotice) => {
    Keyboard.dismiss();
    setError(value);
    setErrorPresented(true);
  }, []);
  const handleStatus = useCallback(
    ({ nativeEvent }: NativeSyntheticEvent<NativeTranscriptStatus>) => {
      onStatus(nativeEvent);
      setNotice(
        nativeEvent.busy || nativeEvent.ready === false
          ? null
          : (nativeEvent.notice ?? null),
      );
      setEmpty(!nativeEvent.hasTurns && !nativeEvent.error);
    },
    [onStatus],
  );
  // The latest error shows in the composer; a tap on one opens its sheet.
  useReportSessionError(conversationId, notice);
  const handleAction = useCallback(
    ({ nativeEvent: action }: NativeSyntheticEvent<Action>) => {
      if (action.type === "error" && action.notice) showError(action.notice);
      else if (action.type === "metrics" && action.metrics) setMetrics(action);
      else if (action.type === "copy" && action.copyText) {
        void Clipboard.setStringAsync(action.copyText)
          .then(() => Haptics.impactAsync(Haptics.ImpactFeedbackStyle.Light))
          .catch(() => {});
      } else if (action.type === "link" && action.url) {
        if (action.url.startsWith("monocode-file:"))
          openFile?.(action.url.slice("monocode-file:".length));
        else if (/^https?:\/\/|^mailto:/i.test(action.url))
          void Linking.openURL(action.url).catch(() => {});
      }
    },
    [openFile, showError],
  );
  useEffect(() => {
    if (!jump || !focused) return;
    jump.setTarget(() => {
      void view.current?.scrollToEnd();
    });
    return () => {
      jump.setTarget(null);
      jump.visible.set(0);
    };
  }, [jump, focused]);
  const scrollState = useCallback(
    ({ nativeEvent }: NativeSyntheticEvent<{ away: boolean }>) => {
      if (focused)
        jump?.visible.set(
          withTiming(nativeEvent.away ? 1 : 0, { duration: 160 }),
        );
    },
    [focused, jump],
  );
  if (!NativeView || !pairing) return null;
  return (
    <View style={styles.fill}>
      {agent && empty ? (
        <View style={StyleSheet.absoluteFill}>
          <EmptySession />
        </View>
      ) : null}
      <NativeView
        ref={view}
        style={styles.fill}
        configuration={configuration}
        pending={JSON.stringify(pending)}
        appearance={appearance}
        active={focused && started}
        topInset={layout.top}
        bottomInset={layout.bottom}
        composerExtra={layout.composerGrowth}
        keyboardOffset={layout.keyboardOffset}
        onStatus={handleStatus}
        onAction={handleAction}
        onScrollState={scrollState}
      />
      {error ? (
        <SessionErrorSheet
          message={error.text}
          isPresented={errorPresented && focused}
          onDismiss={() => setErrorPresented(false)}
        />
      ) : null}
      {metrics?.metrics ? (
        <TurnMetricsSheet
          isPresented={focused}
          onDismiss={() => setMetrics(null)}
          metrics={metrics.metrics}
          elapsedMs={metrics.durationMs ?? null}
          completedAt={metrics.completedAt}
        />
      ) : null}
    </View>
  );
}
const styles = StyleSheet.create({ fill: { flex: 1 } });
