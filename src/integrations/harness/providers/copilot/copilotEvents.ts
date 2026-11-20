import { AcpSubagents } from "../../core/acpSubagents";
import type { HarnessEvent } from "../../core/types";
import { asRecord, eventsFromAcpUpdate } from "../grok/grokProtocol";

const OPEN = "<final_answer>";
const CLOSE = "</final_answer>";
const MAX_SEARCH_RESULT = 64 * 1024;
const CITATION_LINE = /^\s*(?:\/|[A-Za-z]:[\\/]).+:\d+(?:-\d+)?\s*$/;

/** Copilot omits attribution on code-search prose, but includes it on tools. */
export class CopilotEvents {
  private readonly subagents = new AcpSubagents();
  private readonly searches = new Set<string>();
  private pending = "";

  route(params: unknown): HarnessEvent[] {
    const envelope = asRecord(params);
    const update = asRecord(envelope?.update);
    if (!update) return [];
    const meta = asRecord(update._meta);
    const copilot = asRecord(meta?.["github.com/copilot"]);
    const agentId = copilot?.agentId;
    const attributed =
      typeof agentId === "string" && agentId
        ? {
            ...envelope,
            update: {
              ...update,
              _meta: { ...meta, parentToolCallId: agentId },
            },
          }
        : params;
    const events = eventsFromAcpUpdate(attributed).map((event) => {
      if (event.type !== "tool.updated") return event;
      const search = update.title === "search_code_subagent" && !agentId;
      if (search) this.searches.add(event.callId);
      if (["completed", "failed"].includes(event.status ?? ""))
        this.searches.delete(event.callId);
      return search
        ? { ...event, kind: "agent" as const, title: "Search code" }
        : event;
    });
    return this.subagents
      .route(attributed, events)
      .flatMap<HarnessEvent>((event) => {
        if (event.type !== "message.delta") return [event];
        const text = this.filterSearchResult(event.text);
        return text ? [{ ...event, text }] : [];
      });
  }

  /** Preserve incomplete or non-citation XML rather than dropping real answers. */
  flush(): HarnessEvent[] {
    const text = this.pending;
    this.pending = "";
    this.searches.clear();
    return text ? [{ type: "message.delta", text }] : [];
  }

  private filterSearchResult(text: string): string {
    if (!this.pending && this.searches.size === 0) return text;
    let input = this.pending + text;
    this.pending = "";
    let output = "";
    while (input) {
      const start = input.indexOf(OPEN);
      if (start < 0) {
        // Hold only a possible opening tag split across stream chunks.
        let suffix = Math.min(input.length, OPEN.length - 1);
        while (suffix > 0 && !OPEN.startsWith(input.slice(-suffix)))
          suffix -= 1;
        this.pending = suffix ? input.slice(-suffix) : "";
        return output + input.slice(0, input.length - suffix);
      }
      output += input.slice(0, start);
      input = input.slice(start);
      const end = input.indexOf(CLOSE, OPEN.length);
      if (end < 0) {
        // Bound buffering and preserve raw text when an oversized/incomplete
        // block cannot be identified safely as internal search citations.
        if (input.length > MAX_SEARCH_RESULT) return output + input;
        this.pending = input;
        return output;
      }
      const body = input.slice(OPEN.length, end).trim();
      const citations = body.split(/\r?\n/).filter((line) => line.trim());
      // This exact file:line-range payload is the search agent's internal
      // result. Its authoritative copy remains in the completed tool output.
      if (
        !citations.length ||
        !citations.every((line) => CITATION_LINE.test(line))
      ) {
        output += input.slice(0, end + CLOSE.length);
      }
      input = input.slice(end + CLOSE.length);
    }
    return output;
  }
}
