import { router } from "expo-router";

import { WorkspacePicker } from "@/features/connection/WorkspacePicker";

// Scenes whose project or mono is no longer chosen redirect themselves, so
// saving just steps back to Settings.
export default function WorkspaceSettingsScreen() {
  return <WorkspacePicker editing onDone={() => router.back()} />;
}
