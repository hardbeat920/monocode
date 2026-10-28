export type TranscriptJump = { blockId: string; query?: string; token: number };

const pending = new Map<string, TranscriptJump>();
const listeners = new Set<() => void>();
let nextToken = 0;

export function requestTranscriptJump(
  sessionId: string,
  blockId: string,
  query?: string,
): void {
  pending.set(sessionId, { blockId, query, token: ++nextToken });
  for (const listener of listeners) listener();
}

export function peekTranscriptJump(sessionId: string): TranscriptJump | null {
  return pending.get(sessionId) ?? null;
}

export function clearTranscriptJump(sessionId: string, token: number): void {
  if (pending.get(sessionId)?.token !== token) return;
  pending.delete(sessionId);
  for (const listener of listeners) listener();
}

export function subscribeTranscriptJump(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}
