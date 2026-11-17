import { useCallback, useEffect, useRef, useState, type RefObject } from "react";
import { Alert, type NativeSyntheticEvent } from "react-native";

import { useConnection } from "@/features/connection/ConnectionProvider";
import { makeCommandId } from "@/features/connection/nativeLink";
import { useWorkspace } from "@/features/workspace/WorkspaceProvider";
import { useScheme } from "@/ui/theme";

import {
  DraftController,
  type DraftCardItem,
  type DraftPress,
  type DraftSurface,
} from "./DraftController";

/** What the iOS transcript's cards can be told. */
export type DraftCardsHandle = {
  draftChrome: (id: string, chrome: string) => Promise<void>;
  draftLoad: (id: string, html: string, generation: number) => Promise<void>;
  draftScript: (id: string, script: string) => Promise<void>;
  draftPicture: (id: string, uri: string | null, ratio: number | null) => Promise<void>;
  scrollToDraft: (draft: string) => Promise<void>;
};

/** What they report, by the row each card sits in. */
export type DraftCardEvent = { id: string } & (
  | ({ kind: "shown" | "card" } & DraftCardItem)
  | { kind: "hidden" | "gone" | "lost" | "loadStart" | "touchTaken" }
  | { kind: "loaded"; generation: number }
  | { kind: "message"; data: string; touchedAt: number }
  | { kind: "press"; button: DraftPress }
);

const alert = (title: string, message: string) => Alert.alert(title, message);

/**
 * The draft cards of the iOS transcript: Swift draws them, and each gets a
 * controller here for as long as its row exists. Also says which draft the
 * agent waits on while no card of it is on screen, for the pill that goes there.
 */
export function useDraftCards(
  conversationId: string,
  view: RefObject<DraftCardsHandle | null>,
) {
  const { command } = useConnection();
  const light = useScheme() === "light";
  const { sessions } = useWorkspace();
  const controllers = useRef(new Map<string, DraftController>());
  const [shown, setShown] = useState<ReadonlyMap<string, DraftCardItem>>(new Map());

  const surface = useCallback(
    (id: string): DraftSurface => ({
      chrome: (chrome) => void view.current?.draftChrome(id, JSON.stringify(chrome)),
      load: (html, generation) => void view.current?.draftLoad(id, html, generation),
      script: (script) => void view.current?.draftScript(id, script),
      picture: (uri, size) =>
        void view.current?.draftPicture(id, uri, size ? size.height / size.width : null),
    }),
    [view],
  );

  const onDraft = useCallback(
    ({ nativeEvent: event }: NativeSyntheticEvent<DraftCardEvent>) => {
      const { id } = event;
      const forget = () =>
        setShown((now) => {
          if (!now.has(id)) return now;
          const next = new Map(now);
          next.delete(id);
          return next;
        });
      if (event.kind === "shown") {
        const card = { draft: event.draft, title: event.title };
        let controller = controllers.current.get(id);
        if (!controller) {
          controller = new DraftController({
            conversationId,
            card,
            light,
            command,
            commandId: makeCommandId,
            surface: surface(id),
            alert,
          });
          controllers.current.set(id, controller);
        }
        controller.show();
        setShown((now) => new Map(now).set(id, card));
        return;
      }
      const controller = controllers.current.get(id);
      if (!controller) return;
      switch (event.kind) {
        case "card":
          controller.setCard({ draft: event.draft, title: event.title });
          break;
        case "hidden":
          controller.hide();
          forget();
          break;
        case "gone":
          controller.dispose();
          controllers.current.delete(id);
          forget();
          break;
        case "lost":
          controller.lost();
          break;
        case "loadStart":
          controller.loadStarted();
          break;
        case "loaded":
          controller.loaded(event.generation);
          break;
        case "message":
          controller.message(event.data, event.touchedAt);
          break;
        case "touchTaken":
          controller.touchTaken();
          break;
        case "press":
          controller.press(event.button);
          break;
      }
    },
    [command, conversationId, light, surface],
  );

  useEffect(() => {
    const live = controllers.current;
    return () => {
      for (const controller of live.values()) controller.dispose();
      live.clear();
    };
  }, [conversationId]);

  const waiting = sessions.find((session) => session.id === conversationId)?.waitingDraft;
  const onScreen = !!waiting && [...shown.values()].some((card) => card.draft === waiting);
  return { onDraft, waitingDraft: waiting && !onScreen ? waiting : null };
}
