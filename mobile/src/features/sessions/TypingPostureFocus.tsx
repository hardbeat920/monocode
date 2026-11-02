import { requireNativeView } from "expo";
import { StyleSheet, type ViewProps } from "react-native";

import { hasTypingPosture } from "@/features/connection/nativeLink";

type Props = ViewProps & {
  enabled: boolean;
  onEnterTypingPosture: () => void;
};
const NativeView = hasTypingPosture
  ? requireNativeView<Props>("MonocodeLink", "TypingPostureView")
  : null;

/** A newer native build adds hinge support; older builds simply omit it. */
export function TypingPostureFocus({ enabled, onEnterTypingPosture }: Props) {
  return NativeView ? (
    <NativeView
      enabled={enabled}
      onEnterTypingPosture={onEnterTypingPosture}
      style={StyleSheet.absoluteFill}
      pointerEvents="none"
      accessible={false}
    />
  ) : null;
}
