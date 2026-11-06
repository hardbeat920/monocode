import { isLiquidGlassAvailable } from "expo-glass-effect";

import { useColors } from "./theme";

/**
 * A BottomSheet's `containerColor`. iOS 26 makes a sheet Liquid Glass unless
 * it is given a background, so it gets none there; elsewhere it's the theme's.
 */
export function useSheetColor() {
  const colors = useColors();
  return isLiquidGlassAvailable() ? undefined : colors.sheet;
}
