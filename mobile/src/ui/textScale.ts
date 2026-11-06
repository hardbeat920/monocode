import { useWindowDimensions } from "react-native";

/**
 * How far text may grow inside a control whose height is fixed, such as a
 * segmented switch or a search pill. iOS's own controls stop growing as well.
 */
export const CONTROL_TEXT_MAX = 1.3;

/** The system text size as a multiple of the default: 1 at Large, 3.57 at AX5. */
export function useTextScale() {
  return useWindowDimensions().fontScale ?? 1;
}
