import MaskedView from "@react-native-masked-view/masked-view";
import { Image, StyleSheet, View } from "react-native";

import { MONOCODE_MARK, MONOCODE_MARK_INVERSE } from "@/ui/brand";

/**
 * The Monocode mark cut into the background image, lit from above: a darker
 * floor where it is cut, a shadow under its top edges, and a faint light along
 * its bottom edges. The mark's own translucency sets how deep each part goes.
 */
export function CarvedMark({ size }: { size: number }) {
  const box = { width: size, height: size };
  return (
    <View style={box} accessibilityIgnoresInvertColors>
      {/* The lower lip of the cut catches the light, just below the mark. */}
      <Image
        source={MONOCODE_MARK}
        tintColor="#ffffff"
        style={[StyleSheet.absoluteFill, styles.rim]}
      />
      <Image
        source={MONOCODE_MARK}
        tintColor="#000000"
        style={[StyleSheet.absoluteFill, styles.floor]}
      />
      {/* Everything outside the mark, shifted down and blurred, seen only
          through the mark: the shadow its top edges cast into it. */}
      <MaskedView
        style={StyleSheet.absoluteFill}
        maskElement={<Image source={MONOCODE_MARK} style={box} />}
      >
        <Image
          source={MONOCODE_MARK_INVERSE}
          tintColor="#000000"
          blurRadius={2}
          style={[StyleSheet.absoluteFill, styles.shadow]}
        />
      </MaskedView>
    </View>
  );
}

const styles = StyleSheet.create({
  rim: {
    opacity: 0.18,
    transform: [{ translateY: 1.5 }],
  },
  floor: {
    opacity: 0.45,
  },
  shadow: {
    opacity: 0.8,
    transform: [{ translateY: 3 }],
  },
});
