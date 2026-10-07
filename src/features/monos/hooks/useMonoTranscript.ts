import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  findMonoTranscript,
  getMonoTranscriptPage,
  type MonoTranscriptPage,
} from "../../sessions/data/sessionStore";
import { findTranscriptBlocks } from "../../sessions/model/transcriptFind";
import type { Session } from "../../sessions/model/session";

/** History belongs to the viewer, never to the live session's writable tail. */
export function useMonoTranscript(session: Session, enabled: boolean) {
  const [history, setHistory] = useState<MonoTranscriptPage | null>(null);
  const request = useRef(0);
  const sessionRef = useRef(session);
  sessionRef.current = session;
  useEffect(() => {
    setHistory(null);
    return () => {
      request.current++;
    };
  }, [session.id]);
  const before = history ? history.before : session.monoTranscript?.before;
  const { blocks, historicalBlockIds } = useMemo(() => {
    if (!history)
      return { blocks: session.blocks, historicalBlockIds: undefined };
    if (history.hasNewer)
      return {
        blocks: history.blocks,
        historicalBlockIds: new Set(history.blocks.map((block) => block.id)),
      };
    const liveIds = new Set(session.blocks.map((block) => block.id));
    const historicalBlocks = history.blocks.filter(
      (block) => !liveIds.has(block.id),
    );
    return {
      blocks: [...historicalBlocks, ...session.blocks],
      historicalBlockIds: new Set(historicalBlocks.map((block) => block.id)),
    };
  }, [history, session.blocks]);
  const blocksRef = useRef(blocks);
  blocksRef.current = blocks;

  const loadEarlier = useCallback(async (beforePrepend?: () => void) => {
    if (before == null) return;
    const token = ++request.current;
    const page = await getMonoTranscriptPage(session.id, { before });
    if (token !== request.current) return;
    beforePrepend?.();
    setHistory((previous) => ({
      ...page,
      hasNewer: previous?.hasNewer ?? false,
      blocks: [...page.blocks, ...(previous?.blocks ?? [])],
    }));
  }, [before, session.id]);

  const reveal = useCallback(
    async (blockId: string) => {
      if (!enabled || blocksRef.current.some((block) => block.id === blockId))
        return true;
      const token = ++request.current;
      const page = await getMonoTranscriptPage(session.id, {
        aroundBlockId: blockId,
      });
      if (token !== request.current) return false;
      setHistory(page);
      return true;
    },
    [enabled, session.id],
  );

  const latest = useCallback(() => {
    request.current++;
    setHistory(null);
  }, []);
  const search = useCallback(async (query: string) => {
    const stored = await findMonoTranscript(sessionRef.current.id, query);
    const live = findTranscriptBlocks(sessionRef.current.blocks, query);
    const liveIds = new Set(sessionRef.current.blocks.map((block) => block.id));
    return [...new Set([...stored.filter((id) => !liveIds.has(id)), ...live])];
  }, []);

  return {
    blocks: enabled ? blocks : session.blocks,
    historicalBlockIds: enabled ? historicalBlockIds : undefined,
    hasEarlier: enabled && before != null,
    loadEarlier,
    reveal,
    latest,
    browsingHistory: !!history,
    viewingOlderPage: !!history?.hasNewer,
    search: enabled ? search : undefined,
  };
}
