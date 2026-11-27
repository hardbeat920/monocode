import { Icon } from "@/ui/Icon";

/** A project screen's pages, left to right. Changes and Files are hidden for
 *  now; their views are kept for when they come back. */
export const PROJECT_VIEWS = ["chat", "sessions"] as const;
export type ProjectView = (typeof PROJECT_VIEWS)[number];

/** The project screen's view switch: a new session, then the project's sessions. */
export function projectSegments() {
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
  ];
}
