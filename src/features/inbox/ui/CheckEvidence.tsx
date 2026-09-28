import { openUrl } from "@tauri-apps/plugin-opener";
import { useEffect, useMemo, useState } from "react";
import { FileTypeIcon } from "../../files/ui/FileTypeIcon";
import { AlertCircle, CircleX, ExternalLink } from "../../../shared/ui/icons";
import { gitCommitFileDiff } from "../../../platform/tauri/fs";
import type { GithubCheckDetails } from "../model/githubPrChecks";

type Annotation = GithubCheckDetails["annotations"][number];
type SourceCache = Map<string, Promise<string | null>>;

/** Source previews share reads within a job and always use its checked commit. */
export function CheckEvidence({
  annotations,
  cwd,
  repo,
  headOid,
}: {
  annotations: Annotation[];
  cwd: string;
  repo: string;
  headOid: string;
}) {
  const [showAll, setShowAll] = useState(false);
  const sources = useMemo<SourceCache>(() => new Map(), [cwd, headOid]);
  return (
    <div className="min-w-0 space-y-3">
      {(showAll ? annotations : annotations.slice(0, 5)).map(
        (annotation, index) => (
          <CheckAnnotation
            key={`${headOid}:${annotation.path}:${annotation.line}:${index}`}
            annotation={annotation}
            cwd={cwd}
            repo={repo}
            headOid={headOid}
            sources={sources}
          />
        ),
      )}
      {!showAll && annotations.length > 5 ? (
        <button
          type="button"
          onClick={() => setShowAll(true)}
          className="rounded px-2 py-1 text-[11px] text-content/55 hover:bg-content/5 hover:text-content"
        >
          Show {annotations.length - 5} more annotations
        </button>
      ) : null}
    </div>
  );
}

function CheckAnnotation({
  annotation,
  cwd,
  repo,
  headOid,
  sources,
}: {
  annotation: Annotation;
  cwd: string;
  repo: string;
  headOid: string;
  sources: SourceCache;
}) {
  const relative = annotation.path.replace(/^\.\//, "");
  const validPath =
    Boolean(relative) &&
    !/[\\:\x00-\x1f]/.test(relative) &&
    relative.split("/").every((part) => part && part !== "." && part !== "..");
  const validCommit = /^(?:[a-f0-9]{40}|[a-f0-9]{64})$/i.test(headOid);
  const validLine =
    Number.isSafeInteger(annotation.line) && annotation.line > 0;
  const canRead = Boolean(cwd) && validPath && validCommit && validLine;
  const sourceKey = `${cwd}:${headOid}:${relative}`;
  const [source, setSource] = useState<{
    key: string;
    text: string | null;
  } | null>(null);
  useEffect(() => {
    if (!canRead) return;
    let active = true;
    let request = sources.get(relative);
    if (!request) {
      request = gitCommitFileDiff(cwd, headOid, relative)
        .then((file) =>
          file &&
          !file.binary &&
          !file.tooLarge &&
          typeof file.current === "string"
            ? file.current
            : null,
        )
        .catch(() => null);
      sources.set(relative, request);
    }
    void request.then((text) => {
      if (active) setSource({ key: sourceKey, text });
    });
    return () => {
      active = false;
    };
  }, [canRead, cwd, headOid, relative, sourceKey, sources]);
  const text = source?.key === sourceKey ? source.text : null;
  const lines = text?.split(/\r?\n/);
  const firstLine = Math.max(1, annotation.line - 1);
  const excerpt =
    lines && annotation.line <= lines.length
      ? lines.slice(firstLine - 1, annotation.line + 1)
      : [];
  const validRepo =
    /^[\w.-]+\/[\w.-]+$/.test(repo) &&
    repo.split("/").every((part) => part !== "." && part !== "..");
  const fileUrl =
    validRepo && validPath && validCommit
      ? `https://github.com/${repo}/blob/${headOid}/${relative.split("/").map(encodeURIComponent).join("/")}${validLine ? `#L${annotation.line}` : ""}`
      : null;
  const location = `${annotation.path}${validLine ? `:${annotation.line}` : ""}`;
  const [messageTitle, ...messageLines] = annotation.message.split(/\r?\n/);
  const Mark = annotation.level === "failure" ? CircleX : AlertCircle;
  return (
    <div className="min-w-0 overflow-hidden rounded-lg border border-stroke bg-background-base/35">
      {annotation.path ? (
        <div className="flex min-w-0 items-center gap-1.5 border-b border-stroke bg-content/[0.02] px-3 py-2 text-[12px] text-content/65">
          <FileTypeIcon name={annotation.path} isDir={false} size={13} />
          <span title={location} className="min-w-0 flex-1 truncate">
            {location}
          </span>
          {fileUrl ? (
            <button
              type="button"
              title="View source at the checked commit"
              aria-label={`View ${location} on GitHub`}
              onClick={() => void openUrl(fileUrl)}
              className="-my-1 -mr-1 grid size-6 shrink-0 place-items-center rounded text-content/40 hover:bg-content/5 hover:text-content"
            >
              <ExternalLink className="size-3" strokeWidth={1.75} />
            </button>
          ) : null}
        </div>
      ) : null}
      {excerpt.length ? (
        <div
          className="overflow-x-auto py-2 font-mono text-[11px] leading-5"
          aria-label={`Source at ${headOid}`}
        >
          {excerpt.map((line, index) => (
            <div
              key={index}
              className={`flex min-w-max gap-4 border-l-2 pr-3 ${firstLine + index === annotation.line ? "border-rose-400/40 bg-rose-400/[0.06] text-content/85" : "border-transparent text-content/45"}`}
            >
              <span className="w-8 shrink-0 select-none text-right tabular-nums text-content/30">
                {firstLine + index}
              </span>
              <code className="whitespace-pre">{line || " "}</code>
            </div>
          ))}
        </div>
      ) : null}
      <div
        className={`flex min-w-0 items-start gap-2 px-3 py-3 ${excerpt.length ? "border-t border-stroke" : ""}`}
      >
        <Mark
          className={`mt-0.5 size-3 shrink-0 ${annotation.level === "failure" ? "text-rose-400/70" : "text-amber-400/70"}`}
          strokeWidth={1.75}
        />
        <div className="min-w-0 flex-1">
          <p className="max-h-32 overflow-auto whitespace-pre-wrap break-words text-[12px] text-content/75">
            {messageTitle}
          </p>
          {messageLines.length ? (
            <pre className="mt-1.5 max-h-60 overflow-auto whitespace-pre-wrap break-words font-mono text-[11px] leading-relaxed text-content/65">
              {messageLines.join("\n")}
            </pre>
          ) : null}
          {canRead && source?.key === sourceKey && !excerpt.length ? (
            <p className="mt-2 text-[10px] text-content/40">
              Source preview unavailable for this commit.
            </p>
          ) : null}
        </div>
      </div>
    </div>
  );
}
