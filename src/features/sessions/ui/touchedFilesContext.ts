import { createContext } from "react";

const NO_PATHS: readonly string[] = [];

/** Files the session's tool calls touched, for replies that name one loosely. */
export const TouchedFilesContext = createContext(NO_PATHS);
