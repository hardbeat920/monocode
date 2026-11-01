import { Alert } from "react-native";

import type { SessionSummary } from "@/features/workspace/types";

/** What shows a toast: `useToast`'s `show` and `dismiss`. */
export type ToastSink = {
  show: (message: string, undo?: () => void) => void;
  dismiss: () => void;
};

/** The workspace's side of archiving. `unarchive` is absent where the desktop
 *  can't take an archive back, and then no Undo is offered. */
export type ArchiveDeps = {
  archive: (conversationId: string) => Promise<void>;
  unarchive?: (conversationId: string) => Promise<void>;
};

const reason = (error: unknown) => (error instanceof Error ? error.message : String(error));

/**
 * Archives a session. The workspace takes it off every list at once and puts
 * it back if the desktop refuses (see `archiveSession`); this says so in a
 * toast, with Undo where the desktop can unarchive. An Undo pressed before the
 * archive has landed waits for it, so the two reach the desktop in order.
 */
export async function archiveWithUndo(
  session: Pick<SessionSummary, "id" | "title">,
  deps: ArchiveDeps,
  toast: ToastSink,
) {
  const pending = deps.archive(session.id);
  const { unarchive } = deps;
  toast.show(
    `Archived “${session.title}”`,
    unarchive
      ? () => {
          toast.dismiss();
          pending
            .then(() => unarchive(session.id))
            .catch((error) =>
              toast.show(`Couldn’t undo. “${session.title}” is still archived. ${reason(error)}`),
            );
        }
      : undefined,
  );
  try {
    await pending;
  } catch (error) {
    toast.show(`Couldn’t archive “${session.title}”. It’s back in the list. ${reason(error)}`);
  }
}

/**
 * Archives several sessions, each kept off the lists as it goes, and says how
 * many the desktop took. There is no Undo for the batch.
 */
export async function archiveMany(
  sessions: readonly Pick<SessionSummary, "id">[],
  deps: Pick<ArchiveDeps, "archive">,
  toast: ToastSink,
) {
  const results = await Promise.allSettled(sessions.map((session) => deps.archive(session.id)));
  const failed = results.filter((result) => result.status === "rejected").length;
  const done = results.length - failed;
  toast.show(
    failed
      ? `Archived ${done} of ${results.length}. ${failed} couldn’t be, and are back in the list.`
      : `Archived ${done} ${done === 1 ? "session" : "sessions"}`,
  );
}

/** The ask before an archive that can't be undone; the session toolbar's words. */
export function confirmArchive(session: Pick<SessionSummary, "title">, onConfirm: () => void) {
  Alert.alert(`Archive “${session.title}”?`, "It moves to the archive on your desktop.", [
    { text: "Cancel", style: "cancel" },
    { text: "Archive", isPreferred: true, onPress: onConfirm },
  ]);
}

export function confirmArchiveMany(count: number, from: string, onConfirm: () => void) {
  Alert.alert(
    `Archive ${count} finished ${count === 1 ? "session" : "sessions"} from ${from}?`,
    "They move to the archive on your desktop.",
    [
      { text: "Cancel", style: "cancel" },
      { text: "Archive", isPreferred: true, onPress: onConfirm },
    ],
  );
}
