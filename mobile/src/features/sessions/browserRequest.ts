import { useState } from "react";
import { Alert } from "react-native";

import { useWorkspace } from "@/features/workspace/WorkspaceProvider";

/** The composer's browser request line and its two answers. */
export type BrowserRequest = {
  answering: boolean;
  allow: () => void;
  decline: () => void;
};

/**
 * The desktop's browser Allow bar, for the conversation's composer: there
 * while the desktop says the agent waits, gone once it is answered here,
 * before the next poll catches up. Allow grants the thread, as on desktop;
 * Not now lets the agent go on without it until the next message.
 */
export function useBrowserRequest(
  conversationId: string | null | undefined,
  requested: boolean | undefined,
): BrowserRequest | null {
  const { answerBrowser } = useWorkspace();
  const [answered, setAnswered] = useState(false);
  const [answering, setAnswering] = useState(false);
  // Once the desktop no longer asks, the next request shows again.
  if (answered && !requested) setAnswered(false);
  if (!conversationId || !requested || answered) return null;
  const answer = (allow: boolean) => {
    setAnswering(true);
    answerBrowser(conversationId, allow)
      .then(() => setAnswered(true))
      .catch((reason: unknown) => {
        Alert.alert(
          "Couldn't answer",
          reason instanceof Error ? reason.message : String(reason),
        );
      })
      .finally(() => setAnswering(false));
  };
  return {
    answering,
    allow: () => answer(true),
    decline: () => answer(false),
  };
}
