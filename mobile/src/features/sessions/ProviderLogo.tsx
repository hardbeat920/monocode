import { SvgXml } from "react-native-svg";

import type { Harness } from "@/features/workspace/types";
import { useColors } from "@/ui/theme";

import { MONOCHROME_PROVIDERS, PROVIDER_SVGS } from "./providerSvgs";

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
    <SvgXml
      xml={PROVIDER_SVGS[harness]}
      width={size}
      height={size}
      color={MONOCHROME_PROVIDERS.has(harness) ? color : undefined}
    />
  );
}
