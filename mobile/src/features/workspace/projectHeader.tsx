import { InboxSegmentIcon } from "@/features/inbox/InboxBadge";
import { Icon } from "@/ui/Icon";

/** A project screen's pages, left to right. Changes and Files are hidden for
 *  now; their views are kept for when they come back. */
export const PROJECT_VIEWS = ["chat", "sessions"] as const;
/** With the Inbox, which only a desktop that serves it gets. */
const INBOX_VIEWS = ["chat", "sessions", "inbox"] as const;
export type ProjectView = (typeof INBOX_VIEWS)[number];

/** The pages of a project screen, with the Inbox third where there is one. */
export function projectViews(inbox: boolean): readonly ProjectView[] {
  return inbox ? INBOX_VIEWS : PROJECT_VIEWS;
}

/**
 * The project screen's view switch: a new session, then the project's
 * sessions, then its Inbox (with its unread count) where the desktop has one.
 */
export function projectSegments(inbox?: { unread: number }) {
  return [
    {
      value: "chat" as const,
      label: "New",
      icon: <Icon name="compose" size={18} />,
    },
    {
      value: "sessions" as const,
      label: "Sessions",
      icon: <Icon name="list" size={18} />,
    },
    ...(inbox
      ? [
          {
            value: "inbox" as const,
            label: "Inbox",
            icon: <InboxSegmentIcon unread={inbox.unread} />,
          },
        ]
      : []),
  ];
}
