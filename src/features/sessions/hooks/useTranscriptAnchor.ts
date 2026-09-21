import { useEffect, useState } from "react";
import {
  loadTranscriptAnchor,
  TRANSCRIPT_ANCHOR_CHANGE_EVENT,
} from "../../settings/model/appearance";

/** Subscribes to prompt-to-top changes triggered by saveTranscriptAnchor(). */
export function useTranscriptAnchor(): boolean {
  const [anchor, setAnchor] = useState<boolean>(loadTranscriptAnchor);
  useEffect(() => {
    const onChange = (event: Event) => {
      setAnchor((event as CustomEvent<boolean>).detail === true);
    };
    window.addEventListener(TRANSCRIPT_ANCHOR_CHANGE_EVENT, onChange);
    return () =>
      window.removeEventListener(TRANSCRIPT_ANCHOR_CHANGE_EVENT, onChange);
  }, []);
  return anchor;
}
