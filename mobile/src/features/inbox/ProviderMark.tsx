import { parse, SvgAst, type JsxAST } from "react-native-svg";
import { View } from "react-native";

import { useColors, withAlpha } from "@/ui/theme";

import type { InboxSource } from "./protocol";

// The approved mock's marks, one colour each, drawn in currentColor.
const SVGS: Record<InboxSource, string> = {
  github:
    '<svg viewBox="0 0 16 16" fill="currentColor"><path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38v-1.49c-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82a7.5 7.5 0 014 0c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48v2.2c0 .21.15.46.55.38A8 8 0 0016 8c0-4.42-3.58-8-8-8z"/></svg>',
  linear:
    '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"><circle cx="8" cy="8" r="6.2"/><path d="M3.6 10.2l2.2 2.2M3 7.4l5.6 5.6M5.6 4.4l6 6M9 3.2l3.8 3.8"/></svg>',
  jira: '<svg viewBox="0 0 16 16" fill="currentColor"><path d="M8 1l7 7-3 3-4-4-4 4-3-3z"/><path d="M8 9l3 3-3 3-3-3z" opacity=".6"/></svg>',
  gitlab:
    '<svg viewBox="0 0 16 16" fill="currentColor"><path d="M8 14.5L1.5 9.8 2.6 5.6l2-3.7 1.2 3.700h4.400l1.200-3.700 2 3.700 1.100 4.200z"/></svg>',
  azuredevops:
    '<svg viewBox="0 0 16 16" fill="currentColor"><path d="M15 3v9.500l-4 2.500-5.500-2v2L2.500 11.200 11.500 12V3.700zM11.500 3.700L7 1 4.500 3.200 1 4.300v5.200l2 1V5.700z"/></svg>',
};

export const SOURCE_NAMES: Record<InboxSource, string> = {
  github: "GitHub",
  linear: "Linear",
  jira: "Jira",
  gitlab: "GitLab",
  azuredevops: "Azure DevOps",
};

const TINTS: Record<Exclude<InboxSource, "github">, string> = {
  linear: "#7b84e8",
  jira: "#4c9aff",
  gitlab: "#fc6d26",
  azuredevops: "#3b9be8",
};

/** Each mark, parsed once: a list draws one per row. */
const parsed = new Map<InboxSource, JsxAST | null>();
function markOf(source: InboxSource) {
  // A provider this build doesn't know, from a newer desktop, has no mark.
  const svg = SVGS[source] as string | undefined;
  if (!parsed.has(source)) parsed.set(source, svg ? parse(svg) : null);
  return parsed.get(source) ?? null;
}

/** A provider's colour; GitHub's follows the ink. */
export function useSourceColor(source: InboxSource) {
  const colors = useColors();
  return source === "github" ? colors.content : (TINTS[source] ?? colors.muted);
}

/** The provider's mark on a tinted tile, as in the mock's rows. */
export function ProviderMark({ source, size = 30 }: { source: InboxSource; size?: number }) {
  const tint = useSourceColor(source);
  const ast = markOf(source);
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
      {ast ? (
        <SvgAst ast={ast} override={{ width: size * 0.57, height: size * 0.57, color: tint }} />
      ) : null}
    </View>
  );
}
