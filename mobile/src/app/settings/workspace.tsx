import { useHeaderHeight } from "expo-router/react-navigation";
import { router } from "expo-router";

import { WorkspacePicker } from "@/features/connection/WorkspacePicker";

// Scenes whose project or mono is no longer chosen redirect themselves, so
// saving just steps back to Settings.
export default function WorkspaceSettingsScreen() {
  const headerHeight = useHeaderHeight();
  return (
    <WorkspacePicker
      editing
      headerHeight={headerHeight}
      onDone={() => router.back()}
    />
  );
}
