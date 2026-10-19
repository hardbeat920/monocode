import { useHeaderHeight } from "expo-router/react-navigation";
import { useEffect, useState } from "react";

/** How long to wait for the native header to report before trusting the guess. */
const REPORT_TIMEOUT = 300;

/** The last height a native header reported, so later screens needn't wait. */
let lastReported = 0;

/**
 * The stack's native header height, or 0 until the header has reported it.
 * The stack starts each screen on a guess (a pre-iOS 26, 44pt bar) and only
 * corrects it once the header is laid out natively, so content placed against
 * the first value shifts. Held at the last reported height meanwhile, or 0 on
 * the first screen, which keeps the frame unsettled; if the guess was right
 * and nothing changes, it is taken after a moment.
 */
export function useNativeHeaderHeight() {
  const height = useHeaderHeight();
  const [guess] = useState(height);
  const [waited, setWaited] = useState(false);
  const reported = height !== guess || waited;
  useEffect(() => {
    if (reported) {
      lastReported = height;
      return;
    }
    const timeout = setTimeout(() => setWaited(true), REPORT_TIMEOUT);
    return () => clearTimeout(timeout);
  }, [reported, height]);
  return reported ? height : lastReported;
}
