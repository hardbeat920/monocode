import { useEffect, useMemo, useRef, useState } from "react";
import { innerScrollerTakes } from "../../../shared/hooks/useLockOverscroll";
import { HighlightedCodeBlock } from "./HighlightedCodeBlock";

type CommandResult = {
  command?: string;
  cwd?: string;
  exitCode?: number;
  output?: string;
};

function asObject(value: unknown): Record<string, unknown> | undefined {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : undefined;
}

function firstString(
  record: Record<string, unknown>,
  keys: string[],
): string | undefined {
  for (const key of keys) {
    const value = record[key];
    if (typeof value === "string" && value.trim()) return value;
  }
  return undefined;
}

/** Recognise shell result envelopes without treating arbitrary JSON as a command. */
function commandResult(value: unknown): CommandResult | undefined {
  const record = asObject(value);
  if (!record) return;
  const command = firstString(record, [
    "command_line",
    "commandLine",
    "command",
  ]);
  const cwd = firstString(record, ["workingDir", "working_dir", "cwd"]);
  const rawExit = record.exitCode ?? record.exit_code;
  const exitCode =
    typeof rawExit === "number" && Number.isFinite(rawExit)
      ? rawExit
      : undefined;
  const output =
    firstString(record, [
      "formatted_output",
      "formattedOutput",
      "combined_output",
      "combinedOutput",
    ]) ??
    ([record.stdout ?? record.stdOut, record.stderr ?? record.stdErr]
      .filter((part): part is string => typeof part === "string" && !!part)
      .join("\n") ||
      undefined);
  const hasShellFields = [
    "formatted_output", "formattedOutput", "combined_output", "combinedOutput",
    "stdout", "stdOut", "stderr", "stdErr",
  ].some((key) => Object.prototype.hasOwnProperty.call(record, key));
  if (!command && exitCode === undefined && !hasShellFields) return;
  if (output === undefined && exitCode === undefined) return;
  return { command, cwd, exitCode, output };
}

function commandResults(value: unknown): CommandResult[] | undefined {
  if (Array.isArray(value) && value.length) {
    const commands = value.map(commandResult);
    return commands.every((command) => !!command)
      ? commands as CommandResult[]
      : undefined;
  }
  const record = asObject(value);
  const results = Array.isArray(record?.results) ? record.results : undefined;
  if (results?.length) {
    const commands = results.map(commandResult);
    if (commands.every((command) => !!command))
      return commands as CommandResult[];
  }
  const single = commandResult(value);
  return single ? [single] : undefined;
}

/**
 * Both ACP and the session reducer cap serialized output at 8k characters
 * before adding "\n…". If a command's output is long, this cuts off the JSON
 * string itself. Recover only a recognisable shell-result envelope; the
 * original truncated payload remains available under "View raw JSON".
 */
function cappedCommandResults(output: string): CommandResult[] | undefined {
  const source = output.trimEnd();
  if (!source.endsWith("\n…") || !/^[\[{]/.test(source)) return;
  const prefix = source.slice(0, -2);
  const closers: string[] = [];
  let inString = false;
  let escaped = false;
  for (const char of prefix) {
    if (inString) {
      if (escaped) escaped = false;
      else if (char === "\\") escaped = true;
      else if (char === '"') inString = false;
    } else if (char === '"') inString = true;
    else if (char === "{") closers.push("}");
    else if (char === "[") closers.push("]");
    else if (char === "}" || char === "]") {
      if (closers.pop() !== char) return;
    }
  }
  if (!closers.length) return;
  const suffix = closers.reverse().join("");
  // A clip might bisect a backslash or Unicode escape near the end of a
  // string. Discard at most the incomplete escape, never earlier result rows.
  for (let trim = 0; trim <= 6; trim++) {
    try {
      const restored = prefix.slice(0, prefix.length - trim) +
        (inString ? '"' : "") + suffix;
      const commands = commandResults(JSON.parse(restored));
      if (commands) return commands;
    } catch {
      // Fall back to the original truncated JSON when it isn't a command.
    }
  }
}

/** Mounted only while open: parsing and highlighting never run on collapsed calls. */
export function ToolResultBody({
  output,
  id,
  title,
  kind,
}: {
  output: string;
  id?: string;
  title?: string;
  kind?: "search" | "read";
}) {
  const [showRaw, setShowRaw] = useState(false);
  const regionRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const region = regionRef.current;
    if (!region) return;
    // At a result's edge, hand the wheel to its activity list first. A result
    // outside a live activity list falls back to the conversation scroller.
    const onWheel = (event: WheelEvent) => {
      if (
        event.ctrlKey ||
        !event.deltaY ||
        Math.abs(event.deltaX) > Math.abs(event.deltaY)
      )
        return;
      if (innerScrollerTakes(region, event)) return;
      const deltaY =
        event.deltaY *
        (event.deltaMode === 1
          ? 16
          : event.deltaMode === 2
            ? region.clientHeight
            : 1);
      const max = region.scrollHeight - region.clientHeight;
      if (
        (deltaY < 0 && region.scrollTop > 1) ||
        (deltaY > 0 && region.scrollTop < max - 1)
      )
        return;
      const parent = region.closest<HTMLElement>(
        ".zen-phase-live, .agent-transcript",
      );
      if (!parent || parent.scrollHeight <= parent.clientHeight) return;
      event.preventDefault();
      parent.scrollTop += deltaY;
    };
    region.addEventListener("wheel", onWheel, { passive: false });
    return () => region.removeEventListener("wheel", onWheel);
  }, []);
  const formatted = useMemo(() => {
    try {
      const json: unknown = JSON.parse(output);
      return {
        text: JSON.stringify(json, null, 2),
        incomplete: false,
        // A read/search result can itself be a JSON source file. Its
        // properties (even stdout/exitCode) are content, not shell metadata.
        commands: kind ? undefined : commandResults(json),
      };
    } catch {
      const commands = kind ? undefined : cappedCommandResults(output);
      return commands
        ? { text: output, incomplete: true, commands }
        : undefined;
    }
  }, [output, kind]);
  const matchSummary = kind === "search" && !formatted?.commands
    ? /^(Found \d+ matches?[^\r\n]*)(?:\r?\n|$)/i.exec(output)
    : null;
  const plainText = matchSummary
    ? output.slice(matchSummary[0].length)
    : output;
  return (
    <div
      ref={regionRef}
      id={id}
      data-tool-result
      role="region"
      aria-label="Tool result"
      tabIndex={0}
      className="agent-markdown my-1 max-h-64 min-w-0 overflow-x-auto overflow-y-auto overscroll-y-auto rounded-md border border-content/10 bg-content/[0.025] p-2.5 text-content/80"
    >
      {title && !formatted?.commands ? (
        <div className="mb-2 flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 border-b border-content/10 pb-1.5">
          <span data-tool-result-title title={title} className="min-w-0 truncate font-mono text-[12px] font-medium text-content/90">
            {title}
          </span>
          {matchSummary ? (
            <span data-tool-result-summary className="shrink-0 rounded bg-content/6 px-1.5 py-0.5 font-mono text-[11px] text-content/60">
              {matchSummary[1]}
            </span>
          ) : null}
        </div>
      ) : null}
      {formatted?.commands ? (
        <div className="flex min-w-0 flex-col gap-3">
          {formatted.commands.map((command, index) => (
            <div key={index} className="min-w-0">
              <div className="flex min-w-0 items-center gap-2 text-[12px]">
                {command.command ? (
                  <span title={command.command} className="min-w-0 flex-1 overflow-x-auto whitespace-pre font-mono font-medium text-content/90">
                    {command.command}
                  </span>
                ) : null}
                {command.exitCode !== undefined ? (
                  <span
                    className={`shrink-0 rounded px-1.5 py-0.5 font-mono text-[11px] ${command.exitCode === 0 ? "bg-content/6 text-content/60" : "bg-red-500/10 text-red-400"}`}
                  >
                    Exit {command.exitCode}
                  </span>
                ) : null}
              </div>
              {command.cwd ? (
                <div title={command.cwd} className="mt-0.5 min-w-0 max-w-full overflow-x-auto whitespace-pre font-mono text-[11px] text-content/45">
                  {command.cwd}
                </div>
              ) : null}
              <pre className="mt-2 min-w-0 max-w-full overflow-x-auto whitespace-pre font-mono text-[12px] leading-5 text-content/80">
                {command.output ?? "(No output)"}
              </pre>
            </div>
          ))}
          {formatted.incomplete ? (
            <span className="text-[11px] text-content/50">Output truncated</span>
          ) : null}
          <button
            type="button"
            aria-expanded={showRaw}
            onClick={() => setShowRaw((value) => !value)}
            className="w-fit rounded px-1 py-0.5 text-left font-mono text-[11px] text-content/50 hover:text-content focus-visible:outline focus-visible:outline-1 focus-visible:outline-content/70"
          >
            {showRaw ? "Hide raw JSON" : "View raw JSON"}
          </button>
          {showRaw ? (
            <pre
              data-tool-result-raw
              className="min-w-0 max-w-full overflow-x-auto whitespace-pre border-t border-content/10 pt-2 font-mono text-[11px] leading-5 text-content/65"
            >
              {formatted.text}
            </pre>
          ) : null}
        </div>
      ) : formatted ? (
        <HighlightedCodeBlock
          code={formatted.text}
          language="json"
          isIncomplete={formatted.incomplete}
          lineNumbers={false}
        />
      ) : (
        <pre data-tool-result-text className={title
          ? "min-w-0 max-w-full overflow-x-auto whitespace-pre font-mono text-[12px] leading-5 text-content/80"
          : "min-w-0 whitespace-pre-wrap break-words font-mono text-[12px] leading-5"}>
          {plainText}
        </pre>
      )}
    </div>
  );
}
