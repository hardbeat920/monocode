import { Button } from "@expo/ui/swift-ui";

import { GlassMenuButton } from "@/ui/GlassMenuButton";

const SIZE = 44;

/**
 * The round ⋯ button at the right of a Mono's header: a native menu to see
 * its details, start its conversation afresh, or delete it. The screen asks
 * before anything that can't be undone.
 */
export function MonoMenu({
  onDetails,
  onReset,
  onDelete,
}: {
  onDetails: () => void;
  onReset: () => void;
  onDelete: () => void;
}) {
  return (
    <GlassMenuButton icon="more" symbol="ellipsis" label="More" size={SIZE}>
      <Button label="View details" systemImage="info.circle" onPress={onDetails} />
      <Button label="Reset conversation" systemImage="arrow.counterclockwise" onPress={onReset} />
      <Button label="Delete Mono" systemImage="trash" role="destructive" onPress={onDelete} />
    </GlassMenuButton>
  );
}
