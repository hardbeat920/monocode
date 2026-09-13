import { useEffect, useState } from "react";
import {
  loadTranscriptCompact,
  TRANSCRIPT_COMPACT_CHANGE_EVENT,
} from "../lib/appearance";

/** Subscribes to compact-transcript changes triggered by saveTranscriptCompact(). */
export function useTranscriptCompact(): boolean {
  const [compact, setCompact] = useState<boolean>(loadTranscriptCompact);
  useEffect(() => {
    const onChange = (event: Event) => {
      setCompact((event as CustomEvent<boolean>).detail === true);
    };
    window.addEventListener(TRANSCRIPT_COMPACT_CHANGE_EVENT, onChange);
    return () =>
      window.removeEventListener(TRANSCRIPT_COMPACT_CHANGE_EVENT, onChange);
  }, []);
  return compact;
}
