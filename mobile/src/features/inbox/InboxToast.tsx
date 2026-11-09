import { UndoToast } from "@/ui/UndoToast";

import { useInbox } from "./InboxProvider";

/** The Inbox's line under the list: what a mark did, with Undo while it can be taken back. */
export function InboxToast({ bottom }: { bottom: number }) {
  const { toast, dismissToast } = useInbox();
  return <UndoToast toast={toast} onDismiss={dismissToast} bottom={bottom} />;
}
