import { useLocalSearchParams } from "expo-router";

import { DiffScreen } from "@/features/changes/DiffScreen";
import { shownDiff } from "@/features/changes/shownDiffs";

// A changed file's diff, presented as a native modal (see the root layout).
export default function DiffRoute() {
  const { id } = useLocalSearchParams<{ id: string }>();
  return <DiffScreen file={shownDiff(id)} />;
}
