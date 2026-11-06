import { useLocalSearchParams } from "expo-router";

import { DiffScreen } from "@/features/changes/DiffScreen";
import { shownDiff, shownReview } from "@/features/changes/shownDiffs";
import { ReviewDiff } from "@/features/inbox/ReviewDiff";

// A changed file's diff, presented as a native modal (see the root layout).
export default function DiffRoute() {
  const { id } = useLocalSearchParams<{ id: string }>();
  const file = shownDiff(id);
  const itemKey = shownReview(id);
  return itemKey && file ? (
    <ReviewDiff file={file} itemKey={itemKey} />
  ) : (
    <DiffScreen file={file} />
  );
}
