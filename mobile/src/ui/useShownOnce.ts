import { useState } from "react";

/**
 * True from the first time `shown` is, and from then on: for mounting a sheet
 * only once it is first opened, so a screen doesn't build its native sheets
 * as it comes in, and keeping it after so it can animate closed.
 */
export function useShownOnce(shown: boolean) {
  const [once, setOnce] = useState(shown);
  if (shown && !once) setOnce(true);
  return once || shown;
}
