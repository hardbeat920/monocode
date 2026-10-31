import { parse, SvgAst, type JsxAST } from "react-native-svg";

import type { Harness } from "@/features/workspace/types";
import { useColors } from "@/ui/theme";

import { MONOCHROME_PROVIDERS, PROVIDER_SVGS } from "./providerSvgs";

/** Each provider's mark, parsed once: a list draws one per row, and SvgXml
 *  would parse its XML again for every row it mounts. */
const parsed = new Map<Harness, JsxAST | null>();
function markOf(harness: Harness) {
  // A provider this build doesn't know, from a newer desktop, has no mark.
  const source = PROVIDER_SVGS[harness] as string | undefined;
  if (!parsed.has(harness)) parsed.set(harness, source ? parse(source) : null);
  return parsed.get(harness) ?? null;
}

/** The provider's mark, as in the desktop HarnessIcon. Single-color marks take `color`. */
export function ProviderLogo({
  harness,
  size = 16,
  color: givenColor,
}: {
  harness: Harness;
  size?: number;
  color?: string;
}) {
  const colors = useColors();
  const color = givenColor ?? colors.content;
  return (
    <SvgAst
      ast={markOf(harness)}
      override={{
        width: size,
        height: size,
        color: MONOCHROME_PROVIDERS.has(harness) ? color : undefined,
      }}
    />
  );
}
