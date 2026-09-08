import type { InstalledUpdate } from "../lib/updateNotice";
import { UpdateRailCard } from "./UpdateRailCard";

export function SidebarUpdateFooter({
  update,
  onOpenWhatsNew,
  onDismissUpdate,
}: {
  update?: InstalledUpdate | null;
  onOpenWhatsNew?: (version: string) => void;
  onDismissUpdate?: () => void;
}) {
  if (!update || !onOpenWhatsNew || !onDismissUpdate) return null;

  return (
    <div className="flex flex-col gap-1.5 p-2 pb-1">
      <UpdateRailCard
        update={update}
        onOpen={onOpenWhatsNew}
        onDismiss={onDismissUpdate}
      />
    </div>
  );
}
