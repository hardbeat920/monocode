import { Redirect, useLocalSearchParams } from "expo-router";
import { useMemo } from "react";
import { ConnectionNotice } from "@/features/connection/ConnectionNotice";
import { hasNativeTranscript } from "@/features/connection/nativeLink";

import { PixelMascot } from "@/features/mascots/PixelMascot";
import { Composer } from "@/features/sessions/Composer";
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
  const mono = monos.find((m) => m.id === monoId);
  const conversationId = mono ? monoConversationId(mono.id) : null;
  const conversation = useConversation(conversationId);
  // Stable, so the memoized turns keep their rows across polls.
  const name = mono?.name ?? "";
  const mascot = mono?.mascot ?? "";
  const color = mono?.color ?? "";
  const agent = useMemo(() => ({ name, mascot, color }), [name, mascot, color]);
  if (!mono || !conversationId) return <Redirect href="/" />;

  return (
    <SessionScreen
      pill={{
        icon: <PixelMascot name={mono.mascot} color={mono.color} status={mono.status} size={28} />,
        label: mono.name,
      }}
      composer={
        <Composer
          key={mono.id}
          context={mono.name}
          branch=""
          model={mono.model ?? ""}
          harness={mono.harness}
          busy={conversation.busy}
          onStop={() => stop(conversationId)}
          onSend={(text, attachments) => sendMessage(conversationId, text, attachments)}
        />
      }
    >
      <ConnectionNotice error={conversation.error} />
      {hasNativeTranscript
        ? <NativeTranscript key={mono.id} conversationId={conversationId} agent={agent} />
        : <MonoTranscript key={mono.id} conversationId={conversationId} agent={agent} />}
    </SessionScreen>
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
    <EmptySession />
  );
}
