import { Redirect, router, useIsFocused, useLocalSearchParams } from "expo-router";
import * as Haptics from "expo-haptics";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Alert } from "react-native";
import { ConnectionNotice } from "@/features/connection/ConnectionNotice";
import { hasNativeTranscript } from "@/features/connection/nativeLink";

import { PixelMascot } from "@/features/mascots/PixelMascot";
import { useMonoModel } from "@/features/models/useMonoModel";
import { useDrawerActions, useDrawerSwipe } from "@/ui/ScenePager";
import { Timed } from "@/ui/Timed";
import { useConnection, useMonoTools } from "@/features/connection/ConnectionProvider";
import { rememberPlace } from "@/features/workspace/lastPlace";
import { markMonoSeen, markMonoUnread } from "@/features/workspace/monoSeen";
import { Composer } from "@/features/sessions/Composer";
import { MonoChoose, MonoMessageActions } from "@/features/sessions/MonoCards";
import { MonoMenu } from "@/features/sessions/MonoMenu";
import { useMonoAccent } from "@/features/workspace/monoAccent";
import { AccentOverride, ACCENTS, useScheme } from "@/ui/theme";
import { QuestionCard } from "@/features/sessions/QuestionCard";
import { EmptySession } from "@/features/sessions/EmptySession";
import { SessionScreen } from "@/features/sessions/SessionScreen";
import { Transcript } from "@/features/sessions/Transcript";
import { NativeTranscript } from "@/features/sessions/NativeTranscript";
import type { TranscriptAgent } from "@/features/sessions/TranscriptTurn";
import {
  monoConversationId,
  useConversation,
  useConversationTurns,
  useWorkspace,
} from "@/features/workspace/WorkspaceProvider";

// A mono has one long-lived conversation, so its screen is that conversation.
export default function MonoScreen() {
  const { monoId } = useLocalSearchParams<{ monoId: string }>();
  const { monos, sendMessage, stop } = useWorkspace();
  useDrawerSwipe();
  const mono = monos.find((m) => m.id === monoId);
  const conversationId = mono ? monoConversationId(mono.id) : null;
  const conversation = useConversation(conversationId);
  // Where the app opens next time.
  const { command, refresh } = useConnection();
  // The older desktop app has no profile, reset or delete for a Mono.
  const tools = useMonoTools();
  const desktop = useConnection().pairing?.desktop.fingerprint;
  const placeId = mono?.id;
  useEffect(() => {
    if (placeId) rememberPlace(desktop, { kind: "mono", id: placeId });
  }, [desktop, placeId]);
  // Its replies are seen as they arrive while its chat is in front, unless
  // it was just marked unread: that holds until the chat is left.
  const focused = useIsFocused();
  const keptUnread = useRef(false);
  useEffect(() => {
    if (!focused) keptUnread.current = false;
    else if (mono && !keptUnread.current) markMonoSeen(desktop, mono);
  }, [focused, desktop, mono]);
  // A Mono's message being replied to, quoted over the next one sent.
  const [reply, setReply] = useState<string | null>(null);
  const drawer = useDrawerActions();
  const messageActions = useMemo(
    () => ({
      reply: setReply,
      // As Messages: the chat reads as new again, and the list comes up.
      markUnread: () => {
        if (!mono) return;
        keptUnread.current = true;
        markMonoUnread(desktop, mono);
        drawer.open();
      },
    }),
    [desktop, mono, drawer],
  );
  // Stable, so the memoized turns keep their rows across polls.
  const name = mono?.name ?? "";
  const mascot = mono?.mascot ?? "";
  const color = mono?.color ?? "";
  const agent = useMemo(() => ({ name, mascot, color }), [name, mascot, color]);
  const { model, harness, effort } = useMonoModel(mono);
  // The colour picked for this Mono's chat, over the app's accent.
  const monoAccent = useMonoAccent(mono?.id);
  const scheme = useScheme();
  const chatAccent = monoAccent === "theme" ? null : ACCENTS[monoAccent][scheme];
  const choose = useCallback(
    (text: string) => (conversationId ? sendMessage(conversationId, text) : Promise.resolve()),
    [conversationId, sendMessage],
  );
  // A reset or delete, then the catalog read again: a deleted Mono's screen
  // leaves for the start once it's gone from there.
  const act = async (run: () => Promise<unknown>) => {
    try {
      await run();
      void Haptics.notificationAsync(Haptics.NotificationFeedbackType.Success);
      await refresh();
    } catch (reason) {
      Alert.alert("Couldn’t do that", reason instanceof Error ? reason.message : String(reason));
    }
  };
  if (!mono || !conversationId) return <Redirect href="/" />;

  return (
    <Timed id="mono screen">
    <SessionScreen
      pill={{
        icon: <PixelMascot name={mono.mascot} color={mono.color} status={mono.status} size={28} />,
        label: mono.name,
        // Its profile: the desktop's details, and its look to edit.
        onPress: tools
          ? () => router.push({ pathname: "/mono-profile/[monoId]", params: { monoId: mono.id } })
          : undefined,
      }}
      // Without them, settings on the right, as on a project.
      action={tools ? undefined : { icon: "settings", label: "Settings", onPress: () => router.push("/settings") }}
      // Its details, a fresh start or deleting it, from ⋯ on the right.
      trailing={
        tools ? <MonoMenu
          onDetails={() =>
            router.push({ pathname: "/mono-profile/[monoId]", params: { monoId: mono.id } })
          }
          // Both clear what can't come back, so each asks first, as the desktop does.
          onReset={() =>
            Alert.alert(
              "Reset conversation?",
              `The chat with ${mono.name} will be cleared. Its soul, memory and habits stay.`,
              [
                { text: "Cancel", style: "cancel" },
                {
                  text: "Reset",
                  style: "destructive",
                  onPress: () => void act(() => command("mono.reset", { monoId: mono.id })),
                },
              ],
            )
          }
          onDelete={() =>
            Alert.alert(
              `Delete ${mono.name}?`,
              "Its scheduled habits stop. Sessions and notes it created stay saved.",
              [
                { text: "Cancel", style: "cancel" },
                {
                  text: "Delete",
                  style: "destructive",
                  onPress: () => void act(() => command("mono.delete", { monoId: mono.id })),
                },
              ],
            )
          }
        /> : undefined
      }
      composer={
        <AccentOverride value={chatAccent}>
          {conversation.question ? (
            <QuestionCard conversationId={conversationId} prompt={conversation.question} />
          ) : null}
          <Composer
            key={mono.id}
            // One line: the header names the Mono, and its model lives in settings.
            compact
            context=""
            branch=""
            model={model}
            effort={effort}
            harness={harness}
            onModelPress={() =>
              router.push({
                pathname: "/models",
                params: { projectId: "", sessionId: conversationId },
              })
            }
            busy={conversation.busy}
            onStop={() => stop(conversationId)}
            reply={reply}
            onClearReply={() => setReply(null)}
            onSend={(text, attachments) => {
              // A reply opens with the message it answers, quoted on one line,
              // so the Mono reads it in context and the chat shows it above.
              const quoted = reply?.replace(/\s+/g, " ").trim();
              setReply(null);
              return sendMessage(
                conversationId,
                quoted ? `> ${quoted.length > 280 ? quoted.slice(0, 279) + "…" : quoted}\n\n${text}` : text,
                attachments,
              );
            }}
          />
        </AccentOverride>
      }
    >
      <ConnectionNotice error={conversation.error} />
      {/* A suggested reply it offers is sent as the user's message. */}
      <AccentOverride value={chatAccent}>
        <MonoChoose value={choose}>
          <MonoMessageActions value={messageActions}>
            {hasNativeTranscript
              ? <NativeTranscript key={mono.id} conversationId={conversationId} agent={agent} />
              : <MonoTranscript key={mono.id} conversationId={conversationId} agent={agent} />}
          </MonoMessageActions>
        </MonoChoose>
      </AccentOverride>
    </SessionScreen>
    </Timed>
  );
}

/** The mono's turns, read here so a streaming reply re-renders only the transcript. */
function MonoTranscript({
  conversationId,
  agent,
}: {
  conversationId: string;
  agent: TranscriptAgent;
}) {
  const { turns, hasOlder, loadOlder } = useConversationTurns(conversationId);
  return turns.length > 0 ? (
    <Transcript turns={turns} agent={agent} hasOlder={hasOlder} loadOlder={loadOlder} />
  ) : (
    <EmptySession mono={agent} />
  );
}
