import { Image, type ImageSourcePropType, StyleSheet } from "react-native";

// The desktop's "Haze" chat background: the image darkens toward the bottom
// and fades out into the background behind the composer. The fade is baked
// into the render (see fadeOut), so this is one plain image, cheap
// to composite as the screen slides with the drawer; a mask and gradients here
// would be drawn off screen every frame it moves.
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
