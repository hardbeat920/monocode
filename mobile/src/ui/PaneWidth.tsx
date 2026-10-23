import { createContext, use } from "react";
import { useWindowDimensions } from "react-native";

/** Width available to the navigator, excluding an open workspace sidebar. */
export const PaneWidth = createContext<number | null>(null);

export function usePaneWidth() {
  const width = use(PaneWidth);
  const window = useWindowDimensions();
  return width ?? window.width;
}
