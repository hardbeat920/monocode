import { createContext } from "react";

/**
 * Holds the spinners and mascots under it still, say in a drawer that is
 * shut or a page swiped out of view, where nobody would see them move.
 */
export const AnimationsPaused = createContext(false);
