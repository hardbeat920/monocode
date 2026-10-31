import { requireNativeView } from "expo";
import * as Clipboard from "expo-clipboard";
import * as Haptics from "expo-haptics";
import Storage from "expo-sqlite/kv-store";
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
import { useCollapsedToolCalls } from "@/features/settings/toolCalls";
import {
  useNativeTranscript,
  type NativeTranscriptStatus,
} from "@/features/workspace/WorkspaceProvider";
import type { TurnMetrics } from "@/features/workspace/types";
import { pixelLayers } from "@/features/mascots/pixelMascots";
import { AccentOverride, mix, useAccent, useColors, useScheme, useTheme } from "@/ui/theme";
import { usePushStarted } from "@/ui/usePushedIn";

import { EmptySession } from "./EmptySession";
import { useOpenLaunchedSession } from "./LaunchedSessionCard";
import { MonoMessageActions, openDocument, useMonoChoose } from "./MonoCards";
import { FileOpener } from "./Markdown";
import { SessionErrorSheet } from "./SessionErrorSheet";
import { useJumpToEnd, useSessionLayout } from "./SessionScreen";
import type { TranscriptAgent } from "./TranscriptTurn";
import { MessageActionsSheet, type HeldMessage } from "./MessageActionsSheet";
import { TurnMetricsSheet } from "./TurnMetricsSheet";
import { useReportSessionError, type SessionErrorNotice } from "./sessionError";

type Handle = { scrollToEnd: () => Promise<void>; selectText: (text: string) => Promise<void> };
type Action = {
  type: "copy" | "metrics" | "error" | "link" | "session" | "choice" | "document" | "message";
  /** A long-pressed message in a Mono's chat: whose it is. */
  role?: "user" | "agent";
  /** The long-pressed message's first row. */
  id?: string;
  url?: string;
  /** A session a Mono started, from its card. */
  sessionId?: string;
  projectId?: string;
  title?: string;
  /** A suggested reply picked from a Mono's choices. */
  text?: string;
  documentId?: string;
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
  onScrollState: (event: NativeSyntheticEvent<{ away: boolean; log?: string }>) => void;
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
  const scheme = useScheme();
  // Classic draws the user's Mono bubbles in ink, unless an accent is picked.
  const theme = useTheme();
  const accent = useAccent();
  // ...and a Mono's chat picked a colour of its own.
  const chatAccent = use(AccentOverride);
  const ink = theme === "mono" && accent === "theme" && !chatAccent;
  const { fontScale, scale: pixelRatio } = useWindowDimensions();
  const [collapsed] = useCollapsedToolCalls();
  const focused = useIsFocused();
  const started = usePushStarted(pushed);
  const openFile = use(FileOpener);
  const openSession = useOpenLaunchedSession();
  const choose = useMonoChoose();
  const jump = useJumpToEnd();
  const view = useRef<Handle>(null);
  const [notice, setNotice] = useState<SessionErrorNotice | null>(null);
  const [error, setError] = useState<SessionErrorNotice | null>(null);
  const [errorPresented, setErrorPresented] = useState(false);
  const [metrics, setMetrics] = useState<Action | null>(null);
  const [empty, setEmpty] = useState(false);
  const [held, setHeld] = useState<HeldMessage | null>(null);
  const [reactions, react] = useReactions(conversationId);
  const messageActions = use(MonoMessageActions);
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
        // A Mono's chat sets the user's bubbles apart: Classic in ink (black
        // on light, off-white on dark), else in the accent, dimmed into the
        // background so it doesn't glare.
        bubble: !agent
          ? colors.bubble
          : ink
            ? scheme === "dark" ? "#ececec" : "#111111"
            : mix(colors.accent, colors.background, scheme === "dark" ? 0.4 : 0.1),
        bubbleText: !agent
          ? colors.content
          : ink
            ? scheme === "dark" ? "#111111" : "#ffffff"
            : scheme === "dark" ? colors.content : "#ffffff",
        surface: colors.surface,
        surfaceRaised: colors.surfaceRaised,
        stroke: colors.stroke,
        faint: colors.faint,
        fileRef: colors.fileRef,
        fileLinks: !!openFile,
        error: colors.deletions,
        rail: colors.rail,
        agent: agent?.name ?? "",
        reactions,
        // The mascot drawn beside a working Mono's status.
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
    [colors, scheme, ink, agent, reactions, openFile, fontScale, pixelRatio, collapsed],
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
      } else if (action.type === "message" && action.text) {
        setHeld({ text: action.text, role: action.role ?? "agent", id: action.id ?? "" });
      } else if (action.type === "choice" && action.text) {
        choose?.(action.text);
      } else if (action.type === "document" && action.documentId) {
        openDocument(action.documentId, action.title);
      } else if (action.type === "session" && action.sessionId && action.projectId) {
        openSession(action.projectId, action.sessionId, action.title);
      } else if (action.type === "link" && action.url) {
        if (action.url.startsWith("monocode-file:"))
          openFile?.(action.url.slice("monocode-file:".length));
        else if (/^https?:\/\/|^mailto:/i.test(action.url))
          void Linking.openURL(action.url).catch(() => {});
      }
    },
    [choose, openFile, openSession, showError],
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
    ({ nativeEvent }: NativeSyntheticEvent<{ away: boolean; log?: string }>) => {
      // Dev builds trace a Mono chat's scroll around a send.
      if (nativeEvent.log != null) {
        if (__DEV__) console.log("[transcript]", nativeEvent.log);
        return;
      }
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
          <EmptySession mono={agent} />
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
      {agent ? (
        <MessageActionsSheet
          message={focused ? held : null}
          onDismiss={() => setHeld(null)}
          onReply={(message) => messageActions?.reply(message.text)}
          onMarkUnread={messageActions?.markUnread}
          onCopy={(message) =>
            void Clipboard.setStringAsync(message.text)
              .then(() => Haptics.impactAsync(Haptics.ImpactFeedbackStyle.Light))
              .catch(() => {})
          }
          onSelect={(message) => void view.current?.selectText(message.text)}
          onReact={(message, emoji) => react(message.id, emoji)}
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

/**
 * Reactions the user put on a Mono's replies, by the reply's first row, kept
 * on this phone only: the Mono doesn't see them. Picking the same one again
 * takes it off.
 */
function useReactions(conversationId: string) {
  const key = `monocode.reactions.${conversationId}`;
  const [reactions, setReactions] = useState<Record<string, string>>(() => {
    try {
      return JSON.parse(Storage.getItemSync(key) ?? "{}") as Record<string, string>;
    } catch {
      return {};
    }
  });
  const react = useCallback(
    (id: string, emoji: string) => {
      if (!id) return;
      setReactions((current) => {
        const { [id]: previous, ...rest } = current;
        const next = previous === emoji ? rest : { ...rest, [id]: emoji };
        void Storage.setItem(key, JSON.stringify(next));
        return next;
      });
    },
    [key],
  );
  return [reactions, react] as const;
}
