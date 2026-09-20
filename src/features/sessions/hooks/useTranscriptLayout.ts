import { useEffect, useState } from "react";
import {
  loadTranscriptLayout,
  TRANSCRIPT_LAYOUT_CHANGE_EVENT,
  type TranscriptLayout,
} from "../../settings/model/appearance";

/** Subscribes to transcript layout changes triggered by saveTranscriptLayout(). */
export function useTranscriptLayout(): TranscriptLayout {
  const [layout, setLayout] = useState<TranscriptLayout>(loadTranscriptLayout);
  useEffect(() => {
    const onChange = (event: Event) => {
      const detail = (event as CustomEvent<TranscriptLayout>).detail;
      setLayout(detail === "chat" ? "chat" : "full");
    };
    window.addEventListener(TRANSCRIPT_LAYOUT_CHANGE_EVENT, onChange);
    return () =>
      window.removeEventListener(TRANSCRIPT_LAYOUT_CHANGE_EVENT, onChange);
  }, []);
  return layout;
}
