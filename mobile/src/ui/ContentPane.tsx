import { createContext, use, useCallback, useState, type ReactNode } from "react";
import { StyleSheet, useWindowDimensions, View, type LayoutChangeEvent } from "react-native";

const ContentWidth = createContext<number | null>(null);

/** Measured locally: a scene can share its window with a sidebar or a sheet. */
export function ContentPane({ children }: { children: ReactNode }) {
  const [width, setWidth] = useState<number | null>(null);
  const measure = useCallback((event: LayoutChangeEvent) => {
    setWidth(event.nativeEvent.layout.width);
  }, []);
  return (
    <View testID="content-pane" style={styles.pane} onLayout={measure}>
      <ContentWidth value={width}>{children}</ContentWidth>
    </View>
  );
}

export function useContentWidth() {
  const measured = use(ContentWidth);
  const { width } = useWindowDimensions();
  // Keep the initial frame usable until the pane has reported its own width.
  return measured ?? width;
}

const styles = StyleSheet.create({
  pane: { flex: 1, minWidth: 0, overflow: "hidden" },
});
