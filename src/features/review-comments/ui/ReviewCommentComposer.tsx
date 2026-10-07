import { useState } from "react";
import { MessageSquarePlus, X } from "../../../shared/ui/icons";
import { Popover, type PopoverAnchor } from "../../../shared/ui/Popover";
import { addReviewComment } from "../model/reviewComments";
import { MOD } from "../../../platform/tauri/platform";

export type ReviewCommentTarget = {
  workspace: string;
  path: string;
  startLine: number;
  endLine: number;
  snippet: string;
  anchor: PopoverAnchor;
  deleted?: boolean;
};

export function ReviewCommentComposer({ target, onDismiss }: { target: ReviewCommentTarget; onDismiss: () => void }) {
  const [body, setBody] = useState("");
  const save = () => {
    if (!body.trim()) return;
    const { anchor: _anchor, ...location } = target;
    addReviewComment({ ...location, body: body.trim() });
    onDismiss();
  };
  const location = target.startLine === 0 ? target.path : target.startLine === target.endLine ? `${target.path}:${target.startLine}` : `${target.path}:${target.startLine}-${target.endLine}`;
  return <Popover anchor={target.anchor} side="top" align="center" gap={6} width={320} onDismiss={onDismiss} role="dialog" aria-label={`Review comment on ${location}`} className="p-2">
    <form onSubmit={(event) => { event.preventDefault(); save(); }}>
      <div className="mb-1.5 flex items-center gap-2 px-0.5"><span className="min-w-0 flex-1 truncate font-mono text-[11px] text-content/55" title={location}>{location}</span><button type="button" onClick={onDismiss} aria-label="Cancel comment" className="grid size-5 place-items-center rounded text-content/45 hover:bg-content/10"><X className="size-3" /></button></div>
      <textarea autoFocus rows={3} value={body} onChange={(event) => setBody(event.target.value)} onKeyDown={(event) => { if ((event.metaKey || event.ctrlKey) && event.key === "Enter") { event.preventDefault(); save(); } }} placeholder="Leave a review comment…" className="max-h-40 min-h-18 w-full resize-y rounded-lg border border-content/10 bg-background-base/70 px-2.5 py-2 text-[13px] leading-5 text-content outline-none placeholder:text-content/35 focus:border-content/20" />
      <div className="mt-2 flex items-center justify-between gap-3"><span className="text-[10px] text-content/35">{MOD}↩ to save</span><button type="submit" disabled={!body.trim()} className="inline-flex h-7 items-center gap-1.5 rounded-md bg-content px-2.5 text-[12px] font-medium text-background-base disabled:opacity-40"><MessageSquarePlus className="size-3.5" />Add to review</button></div>
    </form>
  </Popover>;
}
