import { useEffect, useRef } from "react";
import {
  LOCAL_PREVIEW_EVENT,
  localPreviewScanner,
  type LocalPreview,
} from "../lib/browserPreview";
import type { Session } from "../lib/session";

/** Only new command output can trigger previews; loaded transcripts cannot. */
export function useLocalPreviews(
  sessions: Session[],
  onPreview: (preview: LocalPreview, sessionId?: string) => void,
) {
  const outputs = useRef(
    new Map<
      string,
      { text: string; scan: ReturnType<typeof localPreviewScanner> }
    >(),
  );
  const initialized = useRef(false);
  const knownSessions = useRef(new Set<string>());
  const runningSessions = useRef(new Set<string>());
  const callback = useRef(onPreview);
  useEffect(() => {
    callback.current = onPreview;
  }, [onPreview]);

  useEffect(() => {
    const handler = (event: Event) =>
      onPreview((event as CustomEvent<LocalPreview>).detail);
    window.addEventListener(LOCAL_PREVIEW_EVENT, handler);
    return () => window.removeEventListener(LOCAL_PREVIEW_EVENT, handler);
  }, [onPreview]);

  useEffect(() => {
    const present = new Set<string>();
    for (const session of sessions) {
      const firstSeen = !knownSessions.current.has(session.id);
      knownSessions.current.add(session.id);
      for (const block of session.blocks) {
        const preview = block.tool?.preview;
        if (preview?.kind !== "shell" || !preview.output) continue;
        const key = `${session.id}:${block.id}`;
        present.add(key);
        let previous = outputs.current.get(key);
        if (!previous) {
          previous = {
            text: "",
            scan: localPreviewScanner((url) =>
              callback.current({ cwd: session.cwd, url }, session.id),
            ),
          };
          outputs.current.set(key, previous);
          if (
            !initialized.current ||
            firstSeen ||
            (!session.busy && !runningSessions.current.has(session.id))
          ) {
            previous.text = preview.output;
            continue;
          }
        }
        const delta = preview.output.startsWith(previous.text)
          ? preview.output.slice(previous.text.length)
          : preview.output;
        previous.text = preview.output;
        previous.scan(
          delta,
          !session.busy ||
            ["completed", "failed", "cancelled", "interrupted"].includes(
              block.tool?.status ?? "",
            ),
        );
      }
    }
    for (const key of outputs.current.keys())
      if (!present.has(key)) outputs.current.delete(key);
    for (const id of knownSessions.current)
      if (!sessions.some((session) => session.id === id))
        knownSessions.current.delete(id);
    initialized.current = true;
    runningSessions.current = new Set(
      sessions.filter((session) => session.busy).map((session) => session.id),
    );
  }, [sessions, onPreview]);
}
