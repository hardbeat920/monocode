import { parse, SvgAst, type JsxAST } from "react-native-svg";
import { View } from "react-native";

import { useColors, withAlpha } from "@/ui/theme";

import type { InboxSource } from "./protocol";

// The marks, one colour each, drawn in currentColor. Linear's is the desktop's
// inbox-linear.svg.
const SVGS: Record<InboxSource, string> = {
  github:
    '<svg viewBox="0 0 16 16" fill="currentColor"><path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38v-1.49c-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82a7.5 7.5 0 014 0c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48v2.2c0 .21.15.46.55.38A8 8 0 0016 8c0-4.42-3.58-8-8-8z"/></svg>',
  linear:
    '<svg viewBox="0 0 100 100" fill="currentColor"><path d="M1.22541 61.5228c-.2225-.9485.90748-1.5459 1.59638-.857L39.3342 97.1782c.6889.6889.0915 1.8189-.857 1.5964C20.0515 94.4522 5.54779 79.9485 1.22541 61.5228ZM.00189135 46.8891c-.01764375.2833.08887215.5599.28957165.7606L52.3503 99.7085c.2007.2007.4773.3075.7606.2896 2.3692-.1476 4.6938-.46 6.9624-.9259.7645-.157 1.0301-1.0963.4782-1.6481L2.57595 39.4485c-.55186-.5519-1.49117-.2863-1.648174.4782-.465915 2.2686-.77832 4.5932-.92588465 6.9624ZM4.21093 29.7054c-.16649.3738-.08169.8106.20765 1.1l64.77602 64.776c.2894.2894.7262.3742 1.1.2077 1.7861-.7956 3.5171-1.6927 5.1855-2.684.5521-.328.6373-1.0867.1832-1.5407L8.43566 24.3367c-.45409-.4541-1.21271-.3689-1.54074.1832-.99132 1.6684-1.88843 3.3994-2.68399 5.1855ZM12.6587 18.074c-.3701-.3701-.393-.9637-.0443-1.3541C21.7795 6.45931 35.1114 0 49.9519 0 77.5927 0 100 22.4073 100 50.0481c0 14.8405-6.4593 28.1724-16.7199 37.3375-.3903.3487-.984.3258-1.3542-.0443L12.6587 18.074Z"/></svg>',
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

/** The provider's mark on its own, for a chip or a filter's row. */
export function SourceLogo({ source, size }: { source: InboxSource; size: number }) {
  const tint = useSourceColor(source);
  const ast = markOf(source);
  return ast ? <SvgAst ast={ast} override={{ width: size, height: size, color: tint }} /> : null;
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
