import { buildControlRequest, parseControlResponse } from "./claudeProtocol";

const REQUEST_PREFIX = "monocode_context_";
const REPLY_TIMEOUT_MS = 5000;

export function compactionWindowFromContextUsage(
  value: Record<string, unknown> | null,
): number | null {
  const window = value?.rawMaxTokens;
  return value?.isAutoCompactEnabled === true &&
    typeof window === "number" &&
    Number.isFinite(window) &&
    window > 0
    ? window
    : null;
}

export class ClaudeContextUsage {
  private sequence = 0;
  private pending: string | null = null;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private disposed = false;
  private unsupported = false;

  constructor(
    private readonly send: (request: Record<string, unknown>) => Promise<void>,
    private readonly publish: (window: number | null) => void,
  ) {}

  refresh(): void {
    if (this.disposed || this.unsupported) return;
    this.clearPending();
    const id = `${REQUEST_PREFIX}${++this.sequence}`;
    this.pending = id;
    this.timer = setTimeout(() => this.fail(id), REPLY_TIMEOUT_MS);
    void this.send(
      buildControlRequest(id, { subtype: "get_context_usage" }),
    ).catch(() => this.fail(id));
  }

  handleResponse(rec: Record<string, unknown>): boolean {
    const response = parseControlResponse(rec);
    if (!response?.requestId.startsWith(REQUEST_PREFIX)) return false;
    if (this.disposed || response.requestId !== this.pending) return true;
    this.clearPending();
    if (
      !response.ok &&
      /(?:unknown|unsupported|unrecognized)\s+(?:control\s+)?(?:request|subtype)|get_context_usage.*(?:not supported|not recognized)/i.test(
        response.error ?? "",
      )
    ) {
      this.unsupported = true;
    }
    this.publish(
      response.ok ? compactionWindowFromContextUsage(response.payload) : null,
    );
    return true;
  }

  dispose(): void {
    this.disposed = true;
    this.clearPending();
  }

  private clearPending(): void {
    clearTimeout(this.timer);
    this.timer = undefined;
    this.pending = null;
  }

  private fail(id: string): void {
    if (this.disposed || this.pending !== id) return;
    this.clearPending();
    this.publish(null);
  }
}
