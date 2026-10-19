import { createContext } from "react";

/** Reveal clocks stop even when a pooled transcript stays mounted. */
export const TranscriptVisibility = createContext(true);

/** Bottom-aligned Mono chats already interpolate growth with their scroll spring. */
export const TranscriptPacing = createContext(true);
