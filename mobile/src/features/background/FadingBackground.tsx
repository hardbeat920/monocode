import { Image, type ImageSourcePropType, StyleSheet } from "react-native";

// Static cached artwork with its effect and empty/session fade baked into
// pixels. No animation loop, masks, or per-frame blur on either platform.
export function FadingBackground({
  source,
  opacity,
}: {
  source: ImageSourcePropType;
  opacity: number;
}) {
  return (
    <Image
      source={source}
      resizeMode="cover"
      style={[StyleSheet.absoluteFill, { opacity }]}
    />
  );
}
