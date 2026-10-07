import { useLayoutEffect, useState, useSyncExternalStore } from "react";
import type { EditorView } from "@codemirror/view";
import { Check, MessageSquare, Trash2, X } from "../../../shared/ui/icons";
import { Popover } from "../../../shared/ui/Popover";
import {
  removeReviewComment,
  reviewCommentsSnapshot,
  subscribeReviewComments,
  updateReviewComment,
  type ReviewComment,
} from "../model/reviewComments";

type Bubble = { comment: ReviewComment; top: number };

export function ReviewCommentBubbles({
  path,
  workspace,
  host,
  view,
  revision,
}: {
  path: string;
  workspace: string;
  host: HTMLDivElement | null;
  view: EditorView | null;
  /** Changes whenever the editor document changes, including reflow-only edits. */
  revision: number;
}) {
  const comments = useSyncExternalStore(
    subscribeReviewComments,
    reviewCommentsSnapshot,
    reviewCommentsSnapshot,
  );
  const [bubbles, setBubbles] = useState<Bubble[]>([]);
  const [editing, setEditing] = useState<ReviewComment | null>(null);

  useLayoutEffect(() => {
    if (!host || !view) return;
    const place = () => {
      const hostRect = host.getBoundingClientRect();
      setBubbles(
        comments
          .filter(
            (comment) =>
              comment.workspace === workspace && comment.path === path,
          )
          .flatMap((comment) => {
            const line = Math.min(
              Math.max(1, comment.startLine),
              view.state.doc.lines,
            );
            const coords = view.coordsAtPos(view.state.doc.line(line).from);
            return coords ? [{ comment, top: coords.top - hostRect.top }] : [];
          }),
      );
    };
    place();
    view.scrollDOM.addEventListener("scroll", place, { passive: true });
    window.addEventListener("resize", place);
    return () => {
      view.scrollDOM.removeEventListener("scroll", place);
      window.removeEventListener("resize", place);
    };
  }, [comments, host, path, revision, view, workspace]);

  return (
    <>
      {bubbles.map(({ comment, top }) => (
        <button
          key={comment.id}
          type="button"
          title="Edit review comment"
          aria-label={`Edit review comment on ${comment.path}:${comment.startLine}`}
          onClick={(event) => {
            setEditing(comment);
            event.currentTarget.blur();
          }}
          style={{ top }}
          className="absolute right-2 z-10 grid size-6 -translate-y-0.5 place-items-center rounded-full border border-accent/30 bg-background-base text-accent shadow-sm hover:bg-accent hover:text-background-base"
        >
          <MessageSquare className="size-3.5" />
        </button>
      ))}
      {editing ? (
        <ReviewCommentEditor
          comment={editing}
          anchor={host && view ? anchorForComment(host, view, editing) : null}
          onDismiss={() => setEditing(null)}
        />
      ) : null}
    </>
  );
}

function anchorForComment(host: HTMLDivElement, view: EditorView, comment: ReviewComment) {
  const line = Math.min(Math.max(1, comment.startLine), view.state.doc.lines);
  const coords = view.coordsAtPos(view.state.doc.line(line).from);
  if (!coords) return null;
  const rect = host.getBoundingClientRect();
  return new DOMRect(rect.right - 32, coords.top, 24, Math.max(1, coords.bottom - coords.top));
}

function ReviewCommentEditor({
  comment,
  anchor,
  onDismiss,
}: {
  comment: ReviewComment;
  anchor: DOMRect | null;
  onDismiss: () => void;
}) {
  const [body, setBody] = useState(comment.body);
  const save = () => {
    if (!body.trim()) return;
    updateReviewComment(comment.id, body);
    onDismiss();
  };
  return (
    <Popover anchor={anchor} side="left" align="start" gap={8} width={320} onDismiss={onDismiss} className="p-2">
      <textarea
        autoFocus
        rows={3}
        value={body}
        onChange={(event) => setBody(event.target.value)}
        className="w-full resize-y rounded border border-content/10 bg-transparent px-2 py-1.5 text-[13px] outline-none focus:border-content/25"
      />
      <div className="mt-2 flex items-center justify-between gap-2">
        <button type="button" onClick={() => { removeReviewComment(comment.id); onDismiss(); }} className="inline-flex items-center gap-1 rounded px-2 py-1 text-xs text-red-400 hover:bg-red-400/10"><Trash2 className="size-3.5" />Delete</button>
        <div className="flex gap-1"><button type="button" onClick={onDismiss} className="grid size-7 place-items-center rounded hover:bg-content/10"><X className="size-3.5" /></button><button type="button" disabled={!body.trim()} onClick={save} className="inline-flex h-7 items-center gap-1 rounded bg-content px-2 text-xs text-background-base disabled:opacity-40"><Check className="size-3.5" />Save</button></div>
      </div>
    </Popover>
  );
}
