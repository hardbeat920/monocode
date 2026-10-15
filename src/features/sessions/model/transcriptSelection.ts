export const SELECTABLE_AGENT_RESPONSE_ATTR = "data-selectable-agent-response";

export type TranscriptSelection = {
  text: string;
  rect: DOMRect;
  /** A copy of the selected range, still valid once the page selection clears. */
  range?: Range;
  /** `rect` is the pointer position where the selection ended, not the text. */
  atPointer?: boolean;
};

export type TranscriptSelectionCandidate = {
  text: string;
  collapsed: boolean;
  anchorResponseId: string | null;
  focusResponseId: string | null;
};

export function validateTranscriptSelection(
  candidate: TranscriptSelectionCandidate,
): string | null {
  const text = candidate.text.trim();
  if (!text || candidate.collapsed) return null;
  if (!candidate.anchorResponseId || !candidate.focusResponseId) return null;
  if (candidate.anchorResponseId !== candidate.focusResponseId) return null;
  return text;
}
