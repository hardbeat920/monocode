import { useState } from "react";
import { Bot, Square, Terminal } from "../../../shared/ui/icons";
import type { BackgroundJob } from "../model/session";
import { formatElapsed, useElapsedFrom } from "./AgentTranscript";

/** What the agent still runs in the background, from any turn, above the composer. */
export function BackgroundJobs({
  jobs,
  onStop,
}: {
  jobs?: BackgroundJob[];
  onStop?: (jobId: string) => Promise<void>;
}) {
  const [stopping, setStopping] = useState<ReadonlySet<string>>(new Set());
  if (!jobs?.length) return null;
  const stop = (jobId: string) => {
    if (!onStop) return;
    setStopping((ids) => new Set(ids).add(jobId));
    // The row goes once the agent reports the job ended; a failed stop
    // gives the button back.
    onStop(jobId).catch(() =>
      setStopping((ids) => {
        const next = new Set(ids);
        next.delete(jobId);
        return next;
      }),
    );
  };
  return (
    <div className="px-2 text-content/55" data-background-jobs>
      <div className="relative z-0 rounded-t-[10px] border border-b-0 border-content/10 bg-content/3 px-2 py-1">
        {jobs.map((job, index) => (
          <BackgroundJobRow
            key={job.id}
            job={job}
            divided={index > 0}
            stopping={stopping.has(job.id)}
            onStop={onStop ? () => stop(job.id) : undefined}
          />
        ))}
      </div>
    </div>
  );
}

function BackgroundJobRow({
  job,
  divided,
  stopping,
  onStop,
}: {
  job: BackgroundJob;
  divided: boolean;
  stopping: boolean;
  onStop?: () => void;
}) {
  const elapsed = formatElapsed(useElapsedFrom(job.startedAt, false));
  const Icon = job.command ? Terminal : Bot;
  return (
    <div
      className={`flex min-h-7 items-center gap-2 text-[12px] ${
        divided ? "border-t border-stroke" : ""
      } ${stopping ? "opacity-50" : ""}`}
      title={job.command ? `${job.title}\n${job.command}` : job.title}
    >
      <Icon className="size-3.5 shrink-0" />
      <span className="max-w-[50%] shrink-0 truncate text-content/80">
        {job.title}
      </span>
      <span className="min-w-0 flex-1 truncate font-mono text-[11px] text-content/40">
        {job.command}
      </span>
      <span className="shrink-0 tabular-nums">{elapsed}</span>
      {onStop ? (
        <button
          type="button"
          title="Stop"
          aria-label={`Stop ${job.title}`}
          disabled={stopping}
          onClick={onStop}
          className="grid size-6 shrink-0 place-items-center rounded-md hover:bg-content/10 hover:text-content disabled:opacity-30"
        >
          <Square className="size-2.5 fill-current" strokeWidth={0} />
        </button>
      ) : null}
    </div>
  );
}
