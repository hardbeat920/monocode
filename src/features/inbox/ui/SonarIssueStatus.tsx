import { useState } from "react";
import type { InboxItem } from "../model/githubTasks";
import { transitionSonarIssue } from "../model/sonar";
import { sonarLabel, sonarTransitionLabel } from "../model/sonarLabels";
import { Modal } from "../../../shared/ui/Modal";
import { SecondaryButton } from "../../../shared/ui/SecondaryButton";

export function SonarIssueStatus({
  item,
  status,
  transitions,
  loading,
  onChanged,
}: {
  item: InboxItem;
  status: string;
  transitions: string[];
  loading: boolean;
  onChanged: (item: InboxItem) => void;
}) {
  const [transition, setTransition] = useState<string | null>(null);
  const [comment, setComment] = useState("");
  const [shareFeedback, setShareFeedback] = useState(false);
  const [posting, setPosting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const close = () => {
    if (!posting) setTransition(null);
  };
  const submit = async () => {
    if (!transition || posting) return;
    setPosting(true);
    setError(null);
    try {
      const next = await transitionSonarIssue(
        item,
        transition,
        comment,
        shareFeedback,
      );
      setTransition(null);
      onChanged(next);
    } catch (err) {
      setError(String(err instanceof Error ? err.message : err));
    } finally {
      setPosting(false);
    }
  };
  return (
    <>
      <select
        aria-label="Change issue status"
        value=""
        disabled={loading || posting || !transitions.length}
        onChange={(event) => {
          if (!event.target.value) return;
          setTransition(event.target.value);
          setComment("");
          setShareFeedback(false);
          setError(null);
        }}
        className="h-7 max-w-52 rounded-md border border-content/15 bg-background-base px-2 text-[12px] text-content disabled:opacity-60"
      >
        <option value="">{sonarLabel(status)}</option>
        {transitions.map((value) => (
          <option key={value} value={value}>
            {sonarTransitionLabel(value)}
          </option>
        ))}
      </select>
      {transition ? (
        <Modal
          title={`Change issue status: ${sonarTransitionLabel(transition)}`}
          onClose={close}
          size="md"
        >
          <form
            className="flex flex-col gap-3 px-4 pb-4 pt-3"
            onSubmit={(event) => {
              event.preventDefault();
              void submit();
            }}
          >
            <p className="text-[13px] text-content/75">{item.title}</p>
            <label className="flex flex-col gap-1 text-[12px] text-content/65">
              Status change comment (optional)
              <textarea
                aria-label="Status change comment"
                value={comment}
                onChange={(event) => setComment(event.target.value)}
                disabled={posting}
                rows={5}
                placeholder="Explain why the status should change"
                className="resize-y rounded-md border border-content/15 bg-transparent p-2 text-content outline-none focus:border-content/35"
              />
            </label>
            <label className="flex items-start gap-2 text-[12px] text-content/65">
              <input
                type="checkbox"
                checked={shareFeedback}
                onChange={(event) => setShareFeedback(event.target.checked)}
                disabled={posting}
              />
              Share this comment with Sonar to help improve analyzers
            </label>
            {error ? (
              <p role="alert" className="text-[12px] text-rose-400">
                {error}
              </p>
            ) : null}
            <div className="flex justify-end gap-2">
              <SecondaryButton disabled={posting} onClick={close}>
                Cancel
              </SecondaryButton>
              <SecondaryButton type="submit" disabled={posting}>
                {posting ? "Changing status…" : "Change status"}
              </SecondaryButton>
            </div>
          </form>
        </Modal>
      ) : null}
    </>
  );
}
