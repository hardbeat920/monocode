import {
  useCallback,
  useEffect,
  type Dispatch,
  type MutableRefObject,
  type SetStateAction,
} from "react";
import { message } from "@tauri-apps/plugin-dialog";
import { releaseNotesForVersion } from "../lib/releaseNotes";
import {
  inboxComposerCard,
  type InboxItem,
} from "../lib/githubTasks";
import {
  linearIssueDetails,
  peekLinearIssueDetails,
} from "../lib/linear";
import { linkedWorkItemFromInboxItem } from "../lib/sessionWorkItem";
import { looksLikeProject } from "../lib/recents";
import { newDefaultSession, type Session } from "../lib/session";
import { newTab, type WorkspaceTab } from "../lib/layout";
import {
  ADD_NOTE_TO_CHAT_EVENT,
  type NoteComposerCard,
} from "../lib/notes";
import type { LinkedWorkItemUpdateCard } from "../lib/linkedWorkItemActivity";

export interface SessionBootstrapDeps {
  active: Session | undefined;
  projectCwd: string;
  sessionDefaults: Session | undefined;
  sessionsRef: MutableRefObject<Session[]>;
  setSessions: Dispatch<SetStateAction<Session[]>>;
  setWhatsNewVersion: Dispatch<SetStateAction<string | null>>;
  setActiveTabId: Dispatch<SetStateAction<string>>;
  setComposerFocused: Dispatch<SetStateAction<boolean>>;
  setSearchViewOpen: Dispatch<SetStateAction<boolean>>;
  setInboxViewOpen: Dispatch<SetStateAction<boolean>>;
  setNotesViewOpen: Dispatch<SetStateAction<boolean>>;
  setSidebarTab: Dispatch<SetStateAction<import("../lib/appearance").SidebarTabId>>;
  appendTab: (tab: WorkspaceTab, cwd?: string) => void;
}

export function useSessionBootstrap(deps: SessionBootstrapDeps) {
  const {
    active,
    projectCwd,
    sessionDefaults,
    sessionsRef,
    setSessions,
    setWhatsNewVersion,
    setActiveTabId,
    setComposerFocused,
    setSearchViewOpen,
    setInboxViewOpen,
    setNotesViewOpen,
    setSidebarTab,
    appendTab,
  } = deps;
  const onOpenWhatsNew = useCallback((version: string) => {
    const document = releaseNotesForVersion(version);
    if (!document) {
      void message(
        "Release notes for this version are not available in this build.",
        { title: "MonoCode" },
      );
      return;
    }
    setWhatsNewVersion(document.source.version);
  }, []);

  const onNew = useCallback(() => {
    setSearchViewOpen(false);
    setInboxViewOpen(false);
    setNotesViewOpen(false);
    const cwd = active?.cwd ?? sessionDefaults?.cwd ?? projectCwd;
    const session = newDefaultSession(cwd, sessionDefaults?.runtimeMode);
    const tab = newTab(session.id);
    setSessions((prev) => [...prev, session]);
    appendTab(tab, cwd);
    setActiveTabId(tab.id);
    setComposerFocused(true);
    return session.id;
  }, [
    active?.cwd,
    appendTab,
    sessionDefaults?.cwd,
    sessionDefaults?.runtimeMode,
    projectCwd,
  ]);

  const onStartInboxItem = useCallback(
    async (item: InboxItem, body?: string) => {
      const start = (description?: string) => {
        setInboxViewOpen(false);
        setNotesViewOpen(false);
        setSidebarTab("sessions");
        const cwd =
          item.projectPath || active?.cwd || sessionDefaults?.cwd || projectCwd;
        const ref =
          item.provider === "linear"
            ? item.identifier?.trim() || `#${item.number}`
            : `#${item.number}`;
        const linkedWorkItem = linkedWorkItemFromInboxItem(item);
        const session = {
          ...newDefaultSession(cwd, sessionDefaults?.runtimeMode),
          title: `${ref} ${item.title}`,
          inboxCard: inboxComposerCard(item, description),
          ...(linkedWorkItem ? { linkedWorkItem } : {}),
        };
        const tab = newTab(session.id);
        setSessions((prev) => [...prev, session]);
        appendTab(tab, cwd);
        setActiveTabId(tab.id);
        setComposerFocused(true);
      };

      if (item.provider !== "linear") {
        start();
        return;
      }
      if (!item.id) {
        throw new Error("Missing Linear issue");
      }
      if (body !== undefined) {
        start(body);
        return;
      }
      const cached = peekLinearIssueDetails(item.id);
      if (cached) {
        start(cached.body);
        return;
      }
      const details = await linearIssueDetails(item.id);
      start(details.body);
    },
    [
      active?.cwd,
      appendTab,
      sessionDefaults?.cwd,
      sessionDefaults?.runtimeMode,
      projectCwd,
    ],
  );

  const onAddNoteToChat = useCallback(
    (card: NoteComposerCard) => {
      if (!card.id) return;
      setSearchViewOpen(false);
      setInboxViewOpen(false);
      setNotesViewOpen(false);
      setSidebarTab("sessions");
      const cwd =
        (card.sourceCwd && looksLikeProject(card.sourceCwd)
          ? card.sourceCwd
          : undefined) ||
        active?.cwd ||
        sessionDefaults?.cwd ||
        projectCwd;
      const title = card.title.trim();
      const session = {
        ...newDefaultSession(cwd, sessionDefaults?.runtimeMode),
        ...(title ? { title } : {}),
        noteCard: card,
      };
      const tab = newTab(session.id);
      setSessions((prev) => [...prev, session]);
      appendTab(tab, cwd);
      setActiveTabId(tab.id);
      setComposerFocused(true);
    },
    [
      active?.cwd,
      appendTab,
      sessionDefaults?.cwd,
      sessionDefaults?.runtimeMode,
      projectCwd,
    ],
  );

  useEffect(() => {
    const onAdd = (event: Event) => {
      const card = (event as CustomEvent<NoteComposerCard>).detail;
      if (!card?.id) return;
      onAddNoteToChat(card);
    };
    window.addEventListener(ADD_NOTE_TO_CHAT_EVENT, onAdd);
    return () => window.removeEventListener(ADD_NOTE_TO_CHAT_EVENT, onAdd);
  }, [onAddNoteToChat]);

  const onInboxCardDismiss = useCallback((sessionId: string) => {
    setSessions((prev) =>
      prev.map((session) =>
        session.id === sessionId && session.inboxCard
          ? { ...session, inboxCard: undefined }
          : session,
      ),
    );
  }, []);

  const setLinkedWorkItemUpdateCard = useCallback(
    (
      sessionId: string,
      update: (
        card: LinkedWorkItemUpdateCard | undefined,
      ) => LinkedWorkItemUpdateCard | undefined,
    ) => {
      const previous = sessionsRef.current;
      const next = previous.map((session) => {
        if (session.id !== sessionId) return session;
        const card = update(session.linkedWorkItemUpdateCard);
        return card === session.linkedWorkItemUpdateCard
          ? session
          : { ...session, linkedWorkItemUpdateCard: card };
      });
      if (!next.some((session, index) => session !== previous[index])) return;
      sessionsRef.current = next;
      setSessions(next);
    },
    [],
  );

  const onLinkedWorkItemUpdateCardDismiss = useCallback(
    (sessionId: string) => {
      setLinkedWorkItemUpdateCard(sessionId, () => undefined);
    },
    [setLinkedWorkItemUpdateCard],
  );

  const onNoteCardDismiss = useCallback((sessionId: string) => {
    setSessions((prev) =>
      prev.map((session) =>
        session.id === sessionId && session.noteCard
          ? { ...session, noteCard: undefined }
          : session,
      ),
    );
  }, []);

  const onHandoffCardDismiss = useCallback((sessionId: string) => {
    setSessions((prev) =>
      prev.map((session) =>
        session.id === sessionId && session.handoffCard
          ? { ...session, handoffCard: undefined }
          : session,
      ),
    );
  }, []);

  return {
    onOpenWhatsNew,
    onNew,
    onStartInboxItem,
    onAddNoteToChat,
    onInboxCardDismiss,
    setLinkedWorkItemUpdateCard,
    onLinkedWorkItemUpdateCardDismiss,
    onNoteCardDismiss,
    onHandoffCardDismiss,
  };
}
