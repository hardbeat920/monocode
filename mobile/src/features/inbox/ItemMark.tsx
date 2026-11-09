import { View } from "react-native";

import { Icon, type IconName } from "@/ui/Icon";
import { useColors, withAlpha, type Palette } from "@/ui/theme";

import { ProviderMark } from "./ProviderMark";
import type { InboxItem } from "./protocol";

/** An item's kind and state in GitHub's own terms: the shape says pull request
 *  or issue, the colour open, draft, merged or closed. */
export function itemState(
  item: Pick<InboxItem, "kind" | "status">,
  colors: Palette,
): { icon: IconName; tint: string } {
  if (item.kind === "issue") {
    return item.status === "closed" || item.status === "merged"
      ? { icon: "passing", tint: colors.merged }
      : { icon: "issue", tint: colors.additions };
  }
  switch (item.status) {
    case "merged":
      return { icon: "merge", tint: colors.merged };
    case "closed":
      return { icon: "pullRequestClosed", tint: colors.deletions };
    case "draft":
      return { icon: "pullRequestDraft", tint: colors.muted };
    default:
      return { icon: "pullRequest", tint: colors.additions };
  }
}

/**
 * A row's leading tile. A GitHub item shows what it is, which tells more than
 * the logo every row would repeat; other sources show their mark.
 */
export function ItemMark({
  item,
  size = 30,
}: {
  item: Pick<InboxItem, "source" | "kind" | "status">;
  size?: number;
}) {
  const colors = useColors();
  if (item.source !== "github") return <ProviderMark source={item.source} size={size} />;
  const { icon, tint } = itemState(item, colors);
  return (
    <View
      accessibilityElementsHidden
      style={{
        width: size,
        height: size,
        borderRadius: size * 0.3,
        borderCurve: "continuous",
        alignItems: "center",
        justifyContent: "center",
        backgroundColor: withAlpha(tint, 0.16),
      }}
    >
      <Icon name={icon} size={Math.round(size * 0.6)} color={tint} />
    </View>
  );
}
