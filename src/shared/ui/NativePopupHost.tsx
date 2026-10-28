import { createContext } from "react";

/** A dedicated native popup already owns positioning, clipping, and its shadow. */
export const NativePopupHost = createContext<HTMLElement | null>(null);
