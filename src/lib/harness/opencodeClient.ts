import {
  closeHarnessSse,
  harnessHttp,
  openHarnessSse,
  watchSse,
} from "./child";
import { asRecord } from "./opencodeProtocol";

export class OpenCodeHttpError extends Error {
  readonly status: number;
  readonly body: unknown;

  constructor(status: number, body: unknown, message: string) {
    super(message);
    this.name = status === 404 ? "NotFoundError" : "OpenCodeHttpError";
    this.status = status;
    this.body = body;
  }
}

export type OpenCodeSession = {
  id: string;
  directory?: string;
  title?: string;
};

export type OpenCodePromptPart =
  | { type: "text"; text: string }
  | { type: "file"; mime: string; filename: string; url: string };

export class OpenCodeClient {
  constructor(
    readonly baseUrl: string,
    readonly directory: string,
  ) {}

  async getSession(sessionID: string): Promise<OpenCodeSession> {
    return this.request<OpenCodeSession>("GET", `/session/${enc(sessionID)}`);
  }

  async createSession(input: {
    title?: string;
    permission?: unknown;
  }): Promise<OpenCodeSession> {
    return this.request<OpenCodeSession>("POST", "/session", {
      body: {
        ...(input.title ? { title: input.title } : {}),
        ...(input.permission ? { permission: input.permission } : {}),
      },
    });
  }

  async updateSession(
    sessionID: string,
    body: Record<string, unknown>,
  ): Promise<OpenCodeSession> {
    return this.request<OpenCodeSession>(
      "PATCH",
      `/session/${enc(sessionID)}`,
      {
        body,
      },
    );
  }

  async forkSession(
    sessionID: string,
    directory: string,
  ): Promise<OpenCodeSession> {
    return this.request<OpenCodeSession>(
      "POST",
      `/session/${enc(sessionID)}/fork`,
      {
        query: { directory },
        body: {},
      },
    );
  }

  async abortSession(sessionID: string): Promise<void> {
    await this.request<unknown>("POST", `/session/${enc(sessionID)}/abort`, {
      body: {},
    }).catch(() => undefined);
  }

  async summarizeSession(
    sessionID: string,
    model: { providerID: string; modelID: string },
  ): Promise<void> {
    await this.request<unknown>(
      "POST",
      `/session/${enc(sessionID)}/summarize`,
      {
        body: model,
        timeoutMs: 30 * 60_000,
      },
    );
  }

  async promptAsync(input: {
    sessionID: string;
    model: { providerID: string; modelID: string };
    agent?: string;
    variant?: string;
    parts: OpenCodePromptPart[];
  }): Promise<void> {
    await this.request<unknown>(
      "POST",
      `/session/${enc(input.sessionID)}/prompt_async`,
      {
        body: {
          model: input.model,
          ...(input.agent ? { agent: input.agent } : {}),
          ...(input.variant ? { variant: input.variant } : {}),
          parts: input.parts,
        },
      },
    );
  }

  async prompt(input: {
    sessionID: string;
    model: { providerID: string; modelID: string };
    agent?: string;
    variant?: string;
    parts: OpenCodePromptPart[];
    timeoutMs?: number;
  }): Promise<{ info?: Record<string, unknown>; parts?: unknown[] }> {
    return this.request("POST", `/session/${enc(input.sessionID)}/message`, {
      body: {
        model: input.model,
        ...(input.agent ? { agent: input.agent } : {}),
        ...(input.variant ? { variant: input.variant } : {}),
        parts: input.parts,
      },
      timeoutMs: input.timeoutMs,
    });
  }

  async replyPermission(
    requestID: string,
    reply: "once" | "always" | "reject",
  ): Promise<void> {
    await this.request<unknown>("POST", `/permission/${enc(requestID)}/reply`, {
      body: { reply },
    });
  }

  async replyQuestion(requestID: string, answers: string[][]): Promise<void> {
    await this.request<unknown>("POST", `/question/${enc(requestID)}/reply`, {
      body: { answers },
    });
  }

  async rejectQuestion(requestID: string): Promise<void> {
    await this.request<unknown>("POST", `/question/${enc(requestID)}/reject`, {
      body: {},
    });
  }

  async subscribeEvents(
    sessionId: string,
    onEvent: (event: Record<string, unknown>) => void,
    onEnd?: (error?: string) => void,
  ): Promise<void> {
    const url = this.url("/event");
    watchSse(
      sessionId,
      (data) => {
        const parsed = parseJson(data);
        const rec = asRecord(parsed);
        if (rec) onEvent(rec);
      },
      onEnd,
    );
    await openHarnessSse(sessionId, url, this.headers());
  }

  async closeEvents(sessionId: string): Promise<void> {
    await closeHarnessSse(sessionId).catch(() => undefined);
  }

  private async request<T>(
    method: string,
    path: string,
    opts?: {
      body?: unknown;
      query?: Record<string, string>;
      timeoutMs?: number;
    },
  ): Promise<T> {
    const url = this.url(path, opts?.query);
    const hasBody = opts?.body !== undefined;
    const response = await harnessHttp({
      url,
      method,
      headers: this.headers(hasBody),
      body: hasBody ? JSON.stringify(opts?.body ?? {}) : undefined,
      timeoutMs: opts?.timeoutMs,
    });
    if (response.status === 204 || response.body.trim() === "") {
      if (response.status >= 400) {
        throw new OpenCodeHttpError(
          response.status,
          response.body,
          httpErrorMessage(response.status, response.body),
        );
      }
      return undefined as T;
    }
    const parsed = parseJson(response.body);
    if (response.status >= 400) {
      throw new OpenCodeHttpError(
        response.status,
        parsed,
        httpErrorMessage(response.status, response.body, parsed),
      );
    }
    return unwrapData<T>(parsed);
  }

  private url(path: string, query?: Record<string, string>): string {
    const url = new URL(path, `${this.baseUrl.replace(/\/$/, "")}/`);
    url.searchParams.set("directory", this.directory);
    if (query) {
      for (const [key, value] of Object.entries(query)) {
        url.searchParams.set(key, value);
      }
    }
    return url.toString();
  }

  private headers(json = false): Record<string, string> {
    return {
      ...(json ? { "Content-Type": "application/json" } : {}),
      "x-opencode-directory": encodeURIComponent(this.directory),
    };
  }
}

function enc(value: string): string {
  return encodeURIComponent(value);
}

function parseJson(raw: string): unknown {
  try {
    return JSON.parse(raw) as unknown;
  } catch {
    return raw;
  }
}

function unwrapData<T>(value: unknown): T {
  const rec = asRecord(value);
  if (rec && "data" in rec && rec.data !== undefined) return rec.data as T;
  return value as T;
}

function httpErrorMessage(
  status: number,
  raw: string,
  parsed: unknown = parseJson(raw),
): string {
  const rec = asRecord(parsed);
  const nested = asRecord(rec?.error) ?? asRecord(rec?.data);
  const message =
    (typeof rec?.message === "string" && rec.message.trim()) ||
    (typeof nested?.message === "string" && nested.message.trim()) ||
    raw.trim();
  return message || `OpenCode HTTP ${status}`;
}
